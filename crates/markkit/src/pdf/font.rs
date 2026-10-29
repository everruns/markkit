//! PDF font → Unicode mapping and glyph widths for the text layer.
//!
//! Decision: Unicode comes from, in order, the font's `ToUnicode` CMap, the
//! simple-font encoding (`/Encoding` base + `/Differences` glyph names resolved
//! through an Adobe Glyph List subset), and finally WinAnsi for simple fonts
//! with no usable encoding. Composite (Type0) fonts without `ToUnicode` carry
//! no reliable Unicode mapping; their text is dropped and reported via the
//! `pdf_unmapped_font` warning instead of emitting CID garbage.
//!
//! CMap `bfrange`s are kept as ranges (never expanded) and looked up by binary
//! search, so a `<0000> <FFFFFFFF>` range costs nothing.

use std::collections::HashMap;

use hayro_syntax::object::{Array, Dict, Name, Object, Stream};

use super::guard::{self, Kind};

/// One decoded character code.
pub(crate) struct Glyph {
    pub text: Option<String>,
    /// Advance in text-space units (glyph width / 1000).
    pub width: f32,
    /// Single-byte code 32 (word spacing applies).
    pub is_space: bool,
}

pub(crate) struct Font {
    cmap: Option<CMap>,
    simple: Option<Vec<Option<String>>>,
    composite: bool,
    first_char: u32,
    widths: Vec<f32>,
    cid_widths: Vec<(u32, u32, f32)>,
    default_width: f32,
    /// Composite font with no ToUnicode: text cannot be mapped.
    pub unmapped: bool,
    pub name: String,
    pub bold: bool,
    pub italic: bool,
}

impl Font {
    pub(crate) fn load(dict: &Dict<'_>, cap: usize) -> Self {
        let subtype = name_of(dict.get::<Name<'_>>(b"Subtype"));
        let composite = subtype == "Type0";
        let cmap = dict
            .get::<Stream<'_>>(b"ToUnicode")
            .and_then(|s| decode_stream(&s, cap))
            .map(|d| CMap::parse(&d));
        let name = name_of(dict.get::<Name<'_>>(b"BaseFont"));

        let mut font = Font {
            cmap,
            simple: None,
            composite,
            first_char: 0,
            widths: Vec::new(),
            cid_widths: Vec::new(),
            default_width: 0.5,
            unmapped: false,
            bold: name.to_ascii_lowercase().contains("bold")
                || name.to_ascii_lowercase().contains("black"),
            italic: name.to_ascii_lowercase().contains("italic")
                || name.to_ascii_lowercase().contains("oblique"),
            name,
        };

        if composite {
            let desc = dict
                .get::<Array<'_>>(b"DescendantFonts")
                .and_then(|a| a.iter::<Dict<'_>>().next());
            if let Some(desc) = desc {
                font.default_width = desc.get::<f32>(b"DW").unwrap_or(1000.0) / 1000.0;
                if let Some(w) = desc.get::<Array<'_>>(b"W") {
                    font.cid_widths = parse_cid_widths(&w);
                }
            } else {
                font.default_width = 1.0;
            }
            font.unmapped = font.cmap.as_ref().is_none_or(|c| c.is_empty());
        } else {
            font.simple = Some(simple_encoding(dict));
            font.first_char = dict.get::<u32>(b"FirstChar").unwrap_or(0);
            if let Some(w) = dict.get::<Array<'_>>(b"Widths") {
                font.widths = w
                    .iter::<Object<'_>>()
                    .take(256)
                    .map(|o| o.into_f32().unwrap_or(0.0) / 1000.0)
                    .collect();
            }
            if let Some(mw) = dict
                .get::<Dict<'_>>(b"FontDescriptor")
                .and_then(|d| d.get::<f32>(b"MissingWidth"))
                .filter(|w| *w > 0.0)
            {
                font.default_width = mw / 1000.0;
            }
        }
        font
    }

    /// Fallback for a missing / unloadable font: single-byte WinAnsi.
    pub(crate) fn fallback() -> Self {
        Font {
            cmap: None,
            simple: Some(to_strings(base_table(BaseEncoding::WinAnsi))),
            composite: false,
            first_char: 0,
            widths: Vec::new(),
            cid_widths: Vec::new(),
            default_width: 0.5,
            unmapped: false,
            name: String::new(),
            bold: false,
            italic: false,
        }
    }

    pub(crate) fn decode(&self, bytes: &[u8], out: &mut Vec<Glyph>) {
        let mut i = 0;
        while i < bytes.len() {
            let len = self.code_len(&bytes[i..]);
            let code = bytes[i..i + len]
                .iter()
                .fold(0u32, |a, &b| (a << 8) | b as u32);
            i += len;
            let text = match self.cmap.as_ref().and_then(|c| c.lookup(code)) {
                Some(s) => Some(s),
                None if self.composite => None,
                None => self
                    .simple
                    .as_ref()
                    .and_then(|t| t[(code & 0xFF) as usize].clone()),
            };
            out.push(Glyph {
                text,
                width: self.width(code),
                is_space: len == 1 && code == 32,
            });
        }
    }

    fn code_len(&self, rest: &[u8]) -> usize {
        if let Some(cmap) = &self.cmap
            && !cmap.codespace.is_empty()
        {
            for len in 1..=4.min(rest.len()) {
                let v = rest[..len].iter().fold(0u32, |a, &b| (a << 8) | b as u32);
                if cmap
                    .codespace
                    .iter()
                    .any(|&(l, lo, hi)| l as usize == len && (lo..=hi).contains(&v))
                {
                    return len;
                }
            }
        }
        if self.composite { 2.min(rest.len()) } else { 1 }
    }

    fn width(&self, code: u32) -> f32 {
        if self.composite {
            let i = self.cid_widths.partition_point(|&(lo, _, _)| lo <= code);
            if i > 0 {
                let (lo, hi, w) = self.cid_widths[i - 1];
                if (lo..=hi).contains(&code) {
                    return w;
                }
            }
            return self.default_width;
        }
        code.checked_sub(self.first_char)
            .and_then(|i| self.widths.get(i as usize))
            .copied()
            .filter(|w| *w > 0.0)
            .unwrap_or(self.default_width)
    }
}

pub(crate) fn name_of(n: Option<Name<'_>>) -> String {
    n.map(|n| n.as_str().to_string()).unwrap_or_default()
}

/// Decode a stream through the bounded decoders (never hayro's `decoded()`).
pub(crate) fn decode_stream(s: &Stream<'_>, cap: usize) -> Option<Vec<u8>> {
    let chain: Vec<Kind> = s.filters().into_iter().map(Kind::from_filter).collect();
    if chain.contains(&Kind::Image) {
        return None;
    }
    guard::decode(&chain, &s.raw_data(), cap).ok()
}

/// `/W` array: `c [w1 w2 ...]` and `c_first c_last w` forms, sorted by start.
fn parse_cid_widths(w: &Array<'_>) -> Vec<(u32, u32, f32)> {
    const MAX: usize = 65_536;
    let items: Vec<Object<'_>> = w.iter::<Object<'_>>().take(MAX * 3).collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < items.len() && out.len() < MAX {
        let Some(start) = items[i].clone().into_f32().map(|v| v as u32) else {
            break;
        };
        match items.get(i + 1) {
            Some(Object::Array(arr)) => {
                for (k, o) in arr.iter::<Object<'_>>().take(MAX).enumerate() {
                    let wv = o.into_f32().unwrap_or(0.0) / 1000.0;
                    let c = start.saturating_add(k as u32);
                    out.push((c, c, wv));
                }
                i += 2;
            }
            Some(Object::Number(end)) => {
                let wv = items
                    .get(i + 2)
                    .and_then(|o| o.clone().into_f32())
                    .unwrap_or(0.0)
                    / 1000.0;
                out.push((start, end.as_f64().max(0.0) as u32, wv));
                i += 3;
            }
            _ => break,
        }
    }
    out.sort_by_key(|&(lo, _, _)| lo);
    out
}

// ─── ToUnicode CMap ─────────────────────────────────────────────────

#[derive(Default)]
struct CMap {
    codespace: Vec<(u8, u32, u32)>,
    chars: HashMap<u32, String>,
    /// (lo, hi, dst): dst is the UTF-16 for `lo` (last unit incremented), or an
    /// explicit per-code array.
    ranges: Vec<(u32, u32, RangeDst)>,
}

enum RangeDst {
    Start(Vec<u16>),
    List(Vec<String>),
}

#[derive(Debug, PartialEq)]
enum Tok<'a> {
    Hex(Vec<u8>),
    Word(&'a [u8]),
    Open,
    Close,
}

fn tokens(data: &[u8]) -> Vec<Tok<'_>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < data.len() {
        match data[i] {
            b'%' => {
                while i < data.len() && data[i] != b'\n' && data[i] != b'\r' {
                    i += 1;
                }
            }
            b'<' if data.get(i + 1) == Some(&b'<') => i += 2,
            b'>' if data.get(i + 1) == Some(&b'>') => i += 2,
            b'<' => {
                let end = data[i..].iter().position(|&b| b == b'>').map(|p| i + p);
                let end = end.unwrap_or(data.len());
                let digits: Vec<u8> = data[i + 1..end]
                    .iter()
                    .filter_map(|b| (*b as char).to_digit(16).map(|d| d as u8))
                    .collect();
                let mut bytes: Vec<u8> = digits
                    .chunks(2)
                    .map(|c| c[0] << 4 | c.get(1).copied().unwrap_or(0))
                    .collect();
                if digits.is_empty() {
                    bytes.clear();
                }
                out.push(Tok::Hex(bytes));
                i = end + 1;
            }
            b'[' => {
                out.push(Tok::Open);
                i += 1;
            }
            b']' => {
                out.push(Tok::Close);
                i += 1;
            }
            b if b.is_ascii_whitespace() => i += 1,
            _ => {
                let start = i;
                while i < data.len()
                    && !data[i].is_ascii_whitespace()
                    && !matches!(data[i], b'<' | b'>' | b'[' | b']' | b'%')
                {
                    i += 1;
                }
                if i == start {
                    i += 1;
                } else {
                    out.push(Tok::Word(&data[start..i]));
                }
            }
        }
    }
    out
}

fn be(bytes: &[u8]) -> u32 {
    bytes.iter().take(4).fold(0u32, |a, &b| (a << 8) | b as u32)
}

fn utf16(bytes: &[u8]) -> Vec<u16> {
    bytes
        .chunks(2)
        .map(|c| u16::from_be_bytes([c[0], c.get(1).copied().unwrap_or(0)]))
        .collect()
}

fn utf16_string(units: &[u16]) -> String {
    char::decode_utf16(units.iter().copied())
        .map(|r| r.unwrap_or('\u{FFFD}'))
        .collect()
}

impl CMap {
    fn is_empty(&self) -> bool {
        self.chars.is_empty() && self.ranges.is_empty()
    }

    fn parse(data: &[u8]) -> Self {
        let toks = tokens(data);
        let mut cmap = CMap::default();
        let mut i = 0;
        let hex = |t: Option<&Tok<'_>>| match t {
            Some(Tok::Hex(b)) => Some(b.clone()),
            _ => None,
        };
        while i < toks.len() {
            match &toks[i] {
                Tok::Word(b"begincodespacerange") => {
                    i += 1;
                    while let (Some(lo), Some(hi)) = (hex(toks.get(i)), hex(toks.get(i + 1))) {
                        if (1..=4).contains(&lo.len()) {
                            cmap.codespace.push((lo.len() as u8, be(&lo), be(&hi)));
                        }
                        i += 2;
                    }
                }
                Tok::Word(b"beginbfchar") => {
                    i += 1;
                    while let (Some(src), Some(dst)) = (hex(toks.get(i)), hex(toks.get(i + 1))) {
                        cmap.chars.insert(be(&src), utf16_string(&utf16(&dst)));
                        i += 2;
                    }
                }
                Tok::Word(b"beginbfrange") => {
                    i += 1;
                    while let (Some(lo), Some(hi)) = (hex(toks.get(i)), hex(toks.get(i + 1))) {
                        let (lo, hi) = (be(&lo), be(&hi));
                        match toks.get(i + 2) {
                            Some(Tok::Hex(dst)) => {
                                if hi >= lo && !dst.is_empty() {
                                    cmap.ranges.push((lo, hi, RangeDst::Start(utf16(dst))));
                                }
                                i += 3;
                            }
                            Some(Tok::Open) => {
                                let mut list = Vec::new();
                                i += 3;
                                while let Some(Tok::Hex(d)) = toks.get(i) {
                                    list.push(utf16_string(&utf16(d)));
                                    i += 1;
                                }
                                if toks.get(i) == Some(&Tok::Close) {
                                    i += 1;
                                }
                                if hi >= lo {
                                    cmap.ranges.push((lo, hi, RangeDst::List(list)));
                                }
                            }
                            _ => break,
                        }
                    }
                }
                _ => i += 1,
            }
        }
        cmap.ranges.sort_by_key(|r| r.0);
        cmap
    }

    fn lookup(&self, code: u32) -> Option<String> {
        if let Some(s) = self.chars.get(&code) {
            return Some(s.clone());
        }
        let i = self.ranges.partition_point(|r| r.0 <= code);
        // Ranges may overlap; check a few preceding candidates.
        for (lo, hi, dst) in self.ranges[..i].iter().rev().take(8) {
            if code > *hi {
                continue;
            }
            let off = code - lo;
            return match dst {
                RangeDst::List(l) => l.get(off as usize).cloned(),
                RangeDst::Start(units) => {
                    let mut u = units.clone();
                    let last = u.last_mut()?;
                    *last = last.wrapping_add(off as u16);
                    Some(utf16_string(&u))
                }
            };
        }
        None
    }
}

// ─── Simple-font encodings ──────────────────────────────────────────

#[derive(Clone, Copy)]
enum BaseEncoding {
    Standard,
    WinAnsi,
    MacRoman,
}

fn simple_encoding(dict: &Dict<'_>) -> Vec<Option<String>> {
    let base_of = |n: &str| match n {
        "WinAnsiEncoding" => Some(BaseEncoding::WinAnsi),
        "MacRomanEncoding" => Some(BaseEncoding::MacRoman),
        "StandardEncoding" => Some(BaseEncoding::Standard),
        _ => None,
    };
    // Unknown / absent base: WinAnsi agrees with Standard on printable ASCII and
    // covers the high range that symbolic TrueType fonts usually use.
    let mut base = BaseEncoding::WinAnsi;
    let mut diffs = None;
    if let Some(n) = dict.get::<Name<'_>>(b"Encoding") {
        base = base_of(n.as_str()).unwrap_or(base);
    } else if let Some(enc) = dict.get::<Dict<'_>>(b"Encoding") {
        if let Some(b) = enc
            .get::<Name<'_>>(b"BaseEncoding")
            .and_then(|n| base_of(n.as_str()))
        {
            base = b;
        }
        diffs = enc.get::<Array<'_>>(b"Differences");
    }
    let mut table = to_strings(base_table(base));
    if let Some(diffs) = diffs {
        let mut code: usize = 0;
        for o in diffs.iter::<Object<'_>>().take(1024) {
            match o {
                Object::Number(n) => code = n.as_f64().clamp(0.0, 255.0) as usize,
                Object::Name(n) => {
                    if code < 256 {
                        table[code] = glyph_name_text(n.as_str());
                    }
                    code += 1;
                }
                _ => {}
            }
        }
    }
    table
}

const WIN_ANSI_80: [char; 32] = [
    '€', '\0', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\0', 'Ž', '\0', '\0', '‘',
    '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\0', 'ž', 'Ÿ',
];

const MAC_ROMAN_80: &str = "ÄÅÇÉÑÖÜáàâäãåçéèêëíìîïñóòôöõúùûü†°¢£§•¶ß®©™´¨≠ÆØ∞±≤≥¥µ∂∑∏π∫ªºΩæø¿¡¬√ƒ≈∆«»…\u{A0}ÀÃÕŒœ–—“”‘’÷◊ÿŸ⁄€‹›ﬁﬂ‡·‚„‰ÂÊÁËÈÍÎÏÌÓÔ\u{F8FF}ÒÚÛÙıˆ˜¯˘˙˚¸˝˛ˇ";

const STANDARD_HIGH: &[(u8, char)] = &[
    (0xA1, '¡'),
    (0xA2, '¢'),
    (0xA3, '£'),
    (0xA4, '⁄'),
    (0xA5, '¥'),
    (0xA6, 'ƒ'),
    (0xA7, '§'),
    (0xA8, '¤'),
    (0xA9, '\''),
    (0xAA, '“'),
    (0xAB, '«'),
    (0xAC, '‹'),
    (0xAD, '›'),
    (0xAE, 'ﬁ'),
    (0xAF, 'ﬂ'),
    (0xB1, '–'),
    (0xB2, '†'),
    (0xB3, '‡'),
    (0xB4, '·'),
    (0xB6, '¶'),
    (0xB7, '•'),
    (0xB8, '‚'),
    (0xB9, '„'),
    (0xBA, '”'),
    (0xBB, '»'),
    (0xBC, '…'),
    (0xBD, '‰'),
    (0xBF, '¿'),
    (0xC1, '`'),
    (0xC2, '´'),
    (0xC3, 'ˆ'),
    (0xC4, '˜'),
    (0xC5, '¯'),
    (0xC6, '˘'),
    (0xC7, '˙'),
    (0xC8, '¨'),
    (0xCA, '˚'),
    (0xCB, '¸'),
    (0xCD, '˝'),
    (0xCE, '˛'),
    (0xCF, 'ˇ'),
    (0xD0, '—'),
    (0xE1, 'Æ'),
    (0xE3, 'ª'),
    (0xE8, 'Ł'),
    (0xE9, 'Ø'),
    (0xEA, 'Œ'),
    (0xEB, 'º'),
    (0xF1, 'æ'),
    (0xF5, 'ı'),
    (0xF8, 'ł'),
    (0xF9, 'ø'),
    (0xFA, 'œ'),
    (0xFB, 'ß'),
];

fn base_table(base: BaseEncoding) -> [Option<char>; 256] {
    let mut t = [None; 256];
    for (c, slot) in t.iter_mut().enumerate().take(0x7F).skip(0x20) {
        *slot = Some(c as u8 as char);
    }
    match base {
        BaseEncoding::WinAnsi => {
            for (i, c) in WIN_ANSI_80.iter().enumerate() {
                t[0x80 + i] = (*c != '\0').then_some(*c);
            }
            for (c, slot) in t.iter_mut().enumerate().skip(0xA0) {
                *slot = char::from_u32(c as u32);
            }
        }
        BaseEncoding::MacRoman => {
            for (i, c) in MAC_ROMAN_80.chars().enumerate() {
                t[0x80 + i] = Some(c);
            }
        }
        BaseEncoding::Standard => {
            t[0x27] = Some('’');
            t[0x60] = Some('‘');
            for &(code, c) in STANDARD_HIGH {
                t[code as usize] = Some(c);
            }
        }
    }
    t
}

/// Glyph names for U+0020..U+007E, then U+00A0..U+00FF.
const ASCII_NAMES: [&str; 95] = [
    "space",
    "exclam",
    "quotedbl",
    "numbersign",
    "dollar",
    "percent",
    "ampersand",
    "quotesingle",
    "parenleft",
    "parenright",
    "asterisk",
    "plus",
    "comma",
    "hyphen",
    "period",
    "slash",
    "zero",
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "colon",
    "semicolon",
    "less",
    "equal",
    "greater",
    "question",
    "at",
    "A",
    "B",
    "C",
    "D",
    "E",
    "F",
    "G",
    "H",
    "I",
    "J",
    "K",
    "L",
    "M",
    "N",
    "O",
    "P",
    "Q",
    "R",
    "S",
    "T",
    "U",
    "V",
    "W",
    "X",
    "Y",
    "Z",
    "bracketleft",
    "backslash",
    "bracketright",
    "asciicircum",
    "underscore",
    "grave",
    "a",
    "b",
    "c",
    "d",
    "e",
    "f",
    "g",
    "h",
    "i",
    "j",
    "k",
    "l",
    "m",
    "n",
    "o",
    "p",
    "q",
    "r",
    "s",
    "t",
    "u",
    "v",
    "w",
    "x",
    "y",
    "z",
    "braceleft",
    "bar",
    "braceright",
    "asciitilde",
];

const LATIN1_NAMES: [&str; 96] = [
    "nbspace",
    "exclamdown",
    "cent",
    "sterling",
    "currency",
    "yen",
    "brokenbar",
    "section",
    "dieresis",
    "copyright",
    "ordfeminine",
    "guillemotleft",
    "logicalnot",
    "sfthyphen",
    "registered",
    "macron",
    "degree",
    "plusminus",
    "twosuperior",
    "threesuperior",
    "acute",
    "mu",
    "paragraph",
    "periodcentered",
    "cedilla",
    "onesuperior",
    "ordmasculine",
    "guillemotright",
    "onequarter",
    "onehalf",
    "threequarters",
    "questiondown",
    "Agrave",
    "Aacute",
    "Acircumflex",
    "Atilde",
    "Adieresis",
    "Aring",
    "AE",
    "Ccedilla",
    "Egrave",
    "Eacute",
    "Ecircumflex",
    "Edieresis",
    "Igrave",
    "Iacute",
    "Icircumflex",
    "Idieresis",
    "Eth",
    "Ntilde",
    "Ograve",
    "Oacute",
    "Ocircumflex",
    "Otilde",
    "Odieresis",
    "multiply",
    "Oslash",
    "Ugrave",
    "Uacute",
    "Ucircumflex",
    "Udieresis",
    "Yacute",
    "Thorn",
    "germandbls",
    "agrave",
    "aacute",
    "acircumflex",
    "atilde",
    "adieresis",
    "aring",
    "ae",
    "ccedilla",
    "egrave",
    "eacute",
    "ecircumflex",
    "edieresis",
    "igrave",
    "iacute",
    "icircumflex",
    "idieresis",
    "eth",
    "ntilde",
    "ograve",
    "oacute",
    "ocircumflex",
    "otilde",
    "odieresis",
    "divide",
    "oslash",
    "ugrave",
    "uacute",
    "ucircumflex",
    "udieresis",
    "yacute",
    "thorn",
    "ydieresis",
];

const EXTRA_NAMES: &[(&str, &str)] = &[
    ("quoteleft", "‘"),
    ("quoteright", "’"),
    ("quotedblleft", "“"),
    ("quotedblright", "”"),
    ("quotesinglbase", "‚"),
    ("quotedblbase", "„"),
    ("endash", "–"),
    ("emdash", "—"),
    ("bullet", "•"),
    ("ellipsis", "…"),
    ("dagger", "†"),
    ("daggerdbl", "‡"),
    ("fi", "fi"),
    ("fl", "fl"),
    ("ff", "ff"),
    ("ffi", "ffi"),
    ("ffl", "ffl"),
    ("trademark", "™"),
    ("minus", "−"),
    ("Euro", "€"),
    ("florin", "ƒ"),
    ("perthousand", "‰"),
    ("guilsinglleft", "‹"),
    ("guilsinglright", "›"),
    ("circumflex", "ˆ"),
    ("tilde", "˜"),
    ("OE", "Œ"),
    ("oe", "œ"),
    ("Scaron", "Š"),
    ("scaron", "š"),
    ("Zcaron", "Ž"),
    ("zcaron", "ž"),
    ("Ydieresis", "Ÿ"),
    ("dotlessi", "ı"),
    ("Lslash", "Ł"),
    ("lslash", "ł"),
    ("fraction", "⁄"),
    ("nonbreakingspace", "\u{A0}"),
    ("middot", "·"),
    ("hyphenminus", "-"),
];

/// Map a glyph name to text (AGL subset + `uniXXXX` / `uXXXX` conventions).
fn glyph_name_text(name: &str) -> Option<String> {
    let base = name.split('.').next().unwrap_or(name);
    if base.is_empty() {
        return None;
    }
    if base.contains('_') {
        let parts: Option<String> = base.split('_').map(glyph_name_text).collect();
        return parts;
    }
    if let Some(i) = ASCII_NAMES.iter().position(|n| *n == base) {
        return Some(((0x20 + i) as u8 as char).to_string());
    }
    if let Some(i) = LATIN1_NAMES.iter().position(|n| *n == base) {
        return char::from_u32(0xA0 + i as u32).map(String::from);
    }
    if let Some((_, s)) = EXTRA_NAMES.iter().find(|(n, _)| *n == base) {
        return Some(s.to_string());
    }
    if let Some(hex) = base.strip_prefix("uni")
        && hex.len() >= 4
        && hex.len() % 4 == 0
        && hex.bytes().all(|b| b.is_ascii_hexdigit())
    {
        let units: Vec<u16> = (0..hex.len())
            .step_by(4)
            .filter_map(|i| u16::from_str_radix(&hex[i..i + 4], 16).ok())
            .collect();
        return Some(utf16_string(&units));
    }
    if let Some(hex) = base.strip_prefix('u')
        && (4..=6).contains(&hex.len())
        && hex.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return u32::from_str_radix(hex, 16)
            .ok()
            .and_then(char::from_u32)
            .map(String::from);
    }
    None
}

fn to_strings(t: [Option<char>; 256]) -> Vec<Option<String>> {
    t.iter().map(|c| c.map(String::from)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mac_roman_table_is_complete() {
        assert_eq!(MAC_ROMAN_80.chars().count(), 128);
    }

    #[test]
    fn glyph_names() {
        let g = |n| glyph_name_text(n);
        assert_eq!(g("A").as_deref(), Some("A"));
        assert_eq!(g("eacute").as_deref(), Some("é"));
        assert_eq!(g("quoteright").as_deref(), Some("’"));
        assert_eq!(g("uni00E9").as_deref(), Some("é"));
        assert_eq!(g("uni00410042").as_deref(), Some("AB"));
        assert_eq!(g("u1F600").as_deref(), Some("😀"));
        assert_eq!(g("a.sc").as_deref(), Some("a"));
        assert_eq!(g("fi").as_deref(), Some("fi"));
        assert_eq!(g("g123"), None);
        assert_eq!(glyph_name_text("f_f_i").as_deref(), Some("ffi"));
    }

    #[test]
    fn cmap_bfchar_bfrange_and_codespace() {
        let cmap = CMap::parse(
            b"1 begincodespacerange <0000> <FFFF> endcodespacerange\n\
              2 beginbfchar <0003> <0020> <0024> <00410042> endbfchar\n\
              2 beginbfrange <0010> <0012> <0061> <0020> <0021> [<0058> <D83DDE00>] endbfrange",
        );
        assert_eq!(cmap.codespace, vec![(2, 0, 0xFFFF)]);
        assert_eq!(cmap.lookup(3).as_deref(), Some(" "));
        assert_eq!(cmap.lookup(0x24).as_deref(), Some("AB"));
        assert_eq!(cmap.lookup(0x11).as_deref(), Some("b"));
        assert_eq!(cmap.lookup(0x21).as_deref(), Some("😀"));
        assert_eq!(cmap.lookup(0x13), None);
    }

    #[test]
    fn huge_bfrange_is_not_expanded() {
        let cmap = CMap::parse(b"beginbfrange <00000000> <FFFFFFFF> <0041> endbfrange");
        assert_eq!(cmap.ranges.len(), 1);
        assert_eq!(cmap.lookup(1).as_deref(), Some("B"));
    }

    #[test]
    fn malformed_cmap_never_panics() {
        for s in [
            &b"beginbfchar <"[..],
            b"beginbfrange <00> <01> [",
            b"<<>>[[]]%",
            b"begincodespacerange <> <>",
        ] {
            let _ = CMap::parse(s);
        }
    }

    #[test]
    fn win_ansi_high_range() {
        let t = base_table(BaseEncoding::WinAnsi);
        assert_eq!(t[0x93], Some('“'));
        assert_eq!(t[0xE9], Some('é'));
        assert_eq!(t[0x81], None);
    }
}
