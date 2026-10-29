//! Bounded PDF stream decoding (threat model T8a, decompression bombs).
//!
//! Decision: hayro-syntax decodes object streams and xref streams itself while
//! resolving objects, with no output limit, so a bomb disguised as an object
//! stream could exhaust memory before markkit sees a single page. Before the
//! document is handed to hayro, [`prescan`] finds every `stream` keyword in the
//! raw bytes (streams cannot live inside object streams, so all are visible at
//! top level), reads the filter chain from the stream dictionary, and decodes
//! it with our own bounded decoders, discarding the output. Any stream whose
//! decoded size exceeds `max_pdf_stream_size`, or a total above
//! `max_pdf_decoded_size`, rejects the document. A chain the scan cannot see
//! (indirect `/Filter`, several `/Filter` keys) or an encrypted document (the
//! scan would only see ciphertext) is refused instead of trusted.
//!
//! Content streams, form XObjects and ToUnicode CMaps are decoded through
//! [`decode`] (same bounded decoders), never hayro's unbounded `decoded()`.
//! Image filters (DCT, JPX, JBIG2, CCITT) end a chain: markkit never decodes
//! images. OCR-enabled builds also enable Hayro image codecs: structural streams
//! cannot use image filters, and rendering gets a stricter image preflight.
//! JPEG, header-bounded JPEG2000 and generic image streams are supported; inline images, image
//! DecodeParms, JBIG2/CCITT are refused for OCR until bounded decoding is available.

use hayro_syntax::Filter;

use crate::limits::{LimitExceeded, LimitKind};

/// Raised when a stream would decode past a limit.
#[derive(Debug)]
pub(crate) struct Limit(pub String);

/// Filters the scan understands. `Image` stops a chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    AsciiHex,
    Ascii85,
    Lzw,
    Flate,
    RunLength,
    Image,
}

impl Kind {
    pub(crate) fn from_filter(f: Filter) -> Self {
        match f {
            Filter::AsciiHexDecode => Kind::AsciiHex,
            Filter::Ascii85Decode => Kind::Ascii85,
            Filter::LzwDecode => Kind::Lzw,
            Filter::FlateDecode => Kind::Flate,
            Filter::RunLengthDecode => Kind::RunLength,
            _ => Kind::Image,
        }
    }

    fn from_name(name: &[u8]) -> Option<Self> {
        Some(match name {
            b"ASCIIHexDecode" | b"AHx" => Kind::AsciiHex,
            b"ASCII85Decode" | b"A85" => Kind::Ascii85,
            b"LZWDecode" | b"LZW" => Kind::Lzw,
            b"FlateDecode" | b"Fl" => Kind::Flate,
            b"RunLengthDecode" | b"RL" => Kind::RunLength,
            b"DCTDecode" | b"DCT" | b"JPXDecode" | b"JBIG2Decode" | b"CCITTFaxDecode" | b"CCF"
            | b"Crypt" => Kind::Image,
            _ => return None,
        })
    }
}

/// Decode `data` through `chain`, failing once any stage would produce more
/// than `cap` bytes. Corrupt data yields whatever decoded before the error, like
/// hayro's own lenient decoders.
pub(crate) fn decode(chain: &[Kind], data: &[u8], cap: usize) -> Result<Vec<u8>, Limit> {
    let mut cur = data.to_vec();
    for kind in chain {
        cur = match kind {
            Kind::AsciiHex => ascii_hex(&cur),
            Kind::Ascii85 => ascii85(&cur),
            Kind::Flate => flate(&cur, cap)?,
            Kind::Lzw => lzw(&cur, cap)?,
            Kind::RunLength => run_length(&cur, cap)?,
            Kind::Image => break,
        };
    }
    if cur.len() > cap {
        return Err(over(cap));
    }
    Ok(cur)
}

fn over(cap: usize) -> Limit {
    Limit(format!("PDF stream decodes to more than {cap} bytes"))
}

/// Verify every stream in the file decodes within the limits.
pub(crate) fn prescan(
    bytes: &[u8],
    stream_cap: usize,
    total_cap: usize,
) -> Result<(), PrescanError> {
    prescan_inner(bytes, stream_cap, total_cap, false)
}

#[cfg(all(feature = "pdf-ocr", not(target_arch = "wasm32")))]
pub(crate) fn preflight_render(
    bytes: &[u8],
    stream_cap: usize,
    total_cap: usize,
) -> Result<(), PrescanError> {
    prescan_inner(bytes, stream_cap, total_cap, true)
}

fn prescan_inner(
    bytes: &[u8],
    stream_cap: usize,
    total_cap: usize,
    render: bool,
) -> Result<(), PrescanError> {
    if find_key(bytes, b"/Encrypt").is_some() {
        return Err(PrescanError::Encrypted);
    }
    let mut total = 0usize;
    let mut from = 0;
    while let Some(rel) = find(&bytes[from..], b"stream") {
        let kw = from + rel;
        from = kw + 6;
        // `endstream`, or a word that merely contains "stream".
        if kw > 0 && is_regular(bytes[kw - 1]) {
            continue;
        }
        let data_start = eol_after(bytes, from);
        let dict = dict_before(bytes, kw);
        let chain = match filter_chain(dict) {
            Ok(c) => c,
            Err(()) => return Err(PrescanError::Unverifiable),
        };
        // Enabling the renderer unifies hayro-syntax's image feature. Do not
        // accept declared object/xref streams with image codecs.
        #[cfg(feature = "pdf-ocr")]
        if chain.contains(&Kind::Image)
            && (direct_name(dict, b"/Subtype").as_deref() != Some(b"Image".as_slice())
                || find_key(dict, b"/N").is_some()
                || find_key(dict, b"/First").is_some()
                || matches!(
                    direct_name(dict, b"/Type").as_deref(),
                    Some(b"ObjStm" | b"XRef")
                ))
        {
            return Err(PrescanError::Unverifiable);
        }
        let end = find(&bytes[data_start..], b"endstream")
            .map(|e| data_start + e)
            .unwrap_or(bytes.len());
        let budget = stream_cap.min(total_cap.saturating_sub(total));
        if render && direct_name(dict, b"/Subtype").as_deref() == Some(b"Image".as_slice()) {
            // Reserve conservatively for color conversion, masks and 16-bit
            // samples, not merely the final 1800px destination raster.
            let width = direct_uint(dict, b"/Width").ok_or(PrescanError::Unverifiable)?;
            let height = direct_uint(dict, b"/Height").ok_or(PrescanError::Unverifiable)?;
            let size = width.checked_mul(height).and_then(|n| n.checked_mul(16));
            let Some(mut size) = size.filter(|n| width > 0 && height > 0 && *n <= budget) else {
                return Err(PrescanError::Limit(LimitExceeded::new(
                    if budget < stream_cap {
                        LimitKind::MaxPdfDecodedSize
                    } else {
                        LimitKind::MaxPdfStreamSize
                    },
                    "OCR image working storage exceeds PDF decode budget",
                )));
            };
            if find_key(dict, b"/DecodeParms").is_some() {
                return Err(PrescanError::RenderUnsupported);
            }
            if chain.contains(&Kind::Image) {
                match direct_name(dict, b"/Filter").as_deref() {
                    // JPEG patches oversized SOF dimensions to the PDF bounds.
                    Some(b"DCTDecode" | b"DCT") => {}
                    Some(b"JPXDecode") => {
                        let pixels = jpx_pixels(&bytes[data_start..end])
                            .ok_or(PrescanError::Unverifiable)?;
                        let working =
                            pixels
                                .checked_mul(16)
                                .filter(|n| *n <= budget)
                                .ok_or_else(|| {
                                    PrescanError::Limit(LimitExceeded::new(
                                        if budget < stream_cap {
                                            LimitKind::MaxPdfDecodedSize
                                        } else {
                                            LimitKind::MaxPdfStreamSize
                                        },
                                        "JPEG2000 codestream exceeds OCR image working budget",
                                    ))
                                })?;
                        size = size.max(working);
                    }
                    _ => return Err(PrescanError::RenderUnsupported),
                }
                // Working buffers are released page by page; charge cumulative
                // decoded RGBA bytes once, not every codec scratch copy.
                total += size / 4;
                continue;
            }
            // Generic filters still pass through bounded decoding below.
            total += size / 4;
        } else if !chain.is_empty() && chain.iter().all(|kind| *kind == Kind::Image) {
            continue;
        }

        let out = decode(&chain, &bytes[data_start..end], budget).map_err(|_| {
            if budget < stream_cap {
                PrescanError::Limit(LimitExceeded::new(
                    LimitKind::MaxPdfDecodedSize,
                    format!("PDF streams decode to more than {total_cap} bytes in total (max_pdf_decoded_size)"),
                ))
            } else {
                PrescanError::Limit(LimitExceeded::new(
                    LimitKind::MaxPdfStreamSize,
                    format!("{} (max_pdf_stream_size)", over(stream_cap).0),
                ))
            }
        })?;
        if render && direct_name(dict, b"/Subtype").as_deref() != Some(b"Image".as_slice()) {
            let mut instructions = hayro_syntax::content::UntypedIter::new(&out);
            while let Some(instruction) = instructions.next() {
                let op: &[u8] = instruction.operator;
                if op == b"BI" {
                    return Err(PrescanError::RenderUnsupported);
                }
            }
        }
        total = total.saturating_add(out.len());
        if total > total_cap {
            return Err(PrescanError::Limit(LimitExceeded::new(
                LimitKind::MaxPdfDecodedSize,
                "OCR decoded storage exceeds max_pdf_decoded_size",
            )));
        }
    }
    Ok(())
}

#[derive(Debug)]
pub(crate) enum PrescanError {
    Encrypted,
    Unverifiable,
    RenderUnsupported,
    Limit(LimitExceeded),
}

/// Bound JPEG2000 reference grid, tile area, tile count and channel precision
/// directly from SIZ before any codec allocation. JP2 boxes are length-walked,
/// never searched through arbitrary payload bytes.
fn jpx_pixels(mut data: &[u8]) -> Option<usize> {
    if !data.starts_with(b"\xff\x4f") {
        loop {
            let short = u32::from_be_bytes(data.get(..4)?.try_into().ok()?) as usize;
            let kind = data.get(4..8)?;
            let (len, header) = match short {
                0 => (data.len(), 8),
                1 => (
                    usize::try_from(u64::from_be_bytes(data.get(8..16)?.try_into().ok()?)).ok()?,
                    16,
                ),
                n => (n, 8),
            };
            if len < header || len > data.len() {
                return None;
            }
            if kind == b"jp2c" {
                data = &data[header..len];
                break;
            }
            data = &data[len..];
        }
    }
    if !data.starts_with(b"\xff\x4f\xff\x51") {
        return None;
    }
    let read = |i| Some(u32::from_be_bytes(data.get(i..i + 4)?.try_into().ok()?) as usize);
    let (w, h, x, y, tw, th, tx, ty) = (
        read(8)?,
        read(12)?,
        read(16)?,
        read(20)?,
        read(24)?,
        read(28)?,
        read(32)?,
        read(36)?,
    );
    if x >= w || y >= h || tw == 0 || th == 0 || tx > x || ty > y {
        return None;
    }
    let tiles = (w - tx).div_ceil(tw).checked_mul((h - ty).div_ceil(th))?;
    if tiles > 4096 {
        return None;
    }
    let channels = u16::from_be_bytes(data.get(40..42)?.try_into().ok()?) as usize;
    if !(1..=4).contains(&channels) {
        return None;
    }
    let samples = data.get(42..42 + channels * 3)?;
    if samples
        .as_chunks::<3>()
        .0
        .iter()
        .any(|c| (c[0] & 127) >= 16 || c[1] == 0 || c[2] == 0)
    {
        return None;
    }
    Some(w.checked_mul(h)?.max(tw.checked_mul(th)?))
}

/// Conservative direct dictionary scalars. Ambiguous/indirect values fail closed.
fn direct_value<'a>(dict: &'a [u8], key: &[u8]) -> Option<&'a [u8]> {
    let at = find_key(dict, key)?.end;
    if find_key(&dict[at..], key).is_some() {
        return None;
    }
    Some(skip_ws(&dict[at..]))
}
fn direct_name<'a>(dict: &'a [u8], key: &[u8]) -> Option<std::borrow::Cow<'a, [u8]>> {
    let rest = direct_value(dict, key)?.strip_prefix(b"/")?;
    let n = rest
        .iter()
        .position(|b| !is_regular(*b))
        .unwrap_or(rest.len());
    let name = &rest[..n];
    Some(if name.contains(&b'#') {
        std::borrow::Cow::Owned(unescape_name(name))
    } else {
        std::borrow::Cow::Borrowed(name)
    })
}
fn direct_uint(dict: &[u8], key: &[u8]) -> Option<usize> {
    let rest = direct_value(dict, key)?;
    let n = rest
        .iter()
        .position(|b| !b.is_ascii_digit())
        .unwrap_or(rest.len());
    if n == 0 || rest.get(n).is_some_and(|b| is_regular(*b)) {
        return None;
    }
    // An indirect "W 1 0 R" must not pass as width=1.
    if skip_ws(&rest[n..]).first().is_some_and(u8::is_ascii_digit) {
        return None;
    }
    std::str::from_utf8(&rest[..n]).ok()?.parse().ok()
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// PDF regular character (not whitespace, not a delimiter).
fn is_regular(b: u8) -> bool {
    !matches!(
        b,
        b'\0'
            | b'\t'
            | b'\n'
            | b'\x0C'
            | b'\r'
            | b' '
            | b'('
            | b')'
            | b'<'
            | b'>'
            | b'['
            | b']'
            | b'{'
            | b'}'
            | b'/'
            | b'%'
    )
}

/// Match PDF name keys, including #xx escapes. Return the complete raw name
/// range so callers advance past escaped keys correctly. Comparison allocates
/// nothing, even for a hostile name as long as the input.
fn find_key(hay: &[u8], key: &[u8]) -> Option<std::ops::Range<usize>> {
    let mut from = 0;
    while let Some(rel) = hay[from..].iter().position(|b| *b == b'/') {
        let start = from + rel;
        let raw = &hay[start + 1..];
        let len = raw
            .iter()
            .position(|b| !is_regular(*b))
            .unwrap_or(raw.len());
        let raw = &raw[..len];
        let mut offset = 0;
        let equal = key[1..].iter().all(|expected| {
            let Some(mut value) = raw.get(offset).copied() else {
                return false;
            };
            if value == b'#'
                && let (Some(hi), Some(lo)) = (
                    raw.get(offset + 1).and_then(hex),
                    raw.get(offset + 2).and_then(hex),
                )
            {
                value = hi << 4 | lo;
                offset += 2;
            }
            offset += 1;
            value == *expected
        });
        if equal && offset == raw.len() {
            return Some(start..start + 1 + len);
        }
        from = start + 1 + len;
    }
    None
}

/// Start of stream data after `stream`: CRLF, LF or CR; without an EOL the data
/// is assumed to start right away (malformed files must not evade the scan).
fn eol_after(bytes: &[u8], at: usize) -> usize {
    let mut i = at;
    while bytes.get(i) == Some(&b' ') {
        i += 1;
    }
    match bytes.get(i) {
        Some(b'\r') if bytes.get(i + 1) == Some(&b'\n') => i + 2,
        Some(b'\r' | b'\n') => i + 1,
        _ => i,
    }
}

/// Text between the nearest preceding `obj` keyword and `stream` (bounded window).
fn dict_before(bytes: &[u8], kw: usize) -> &[u8] {
    const WINDOW: usize = 64 * 1024;
    let lo = kw.saturating_sub(WINDOW);
    let region = &bytes[lo..kw];
    let start = region
        .windows(3)
        .rposition(|w| w == b"obj")
        .map(|p| p + 3)
        .unwrap_or(0);
    &region[start..]
}

/// Parse `/Filter` from raw dictionary text. `Err` when it cannot be verified.
fn filter_chain(dict: &[u8]) -> Result<Vec<Kind>, ()> {
    let Some(range) = find_key(dict, b"/Filter") else {
        return Ok(Vec::new());
    };
    if find_key(&dict[range.end..], b"/Filter").is_some() {
        return Err(());
    }
    let mut rest = skip_ws(&dict[range.end..]);
    let array = rest.first() == Some(&b'[');
    if array {
        rest = &rest[1..];
    }
    let mut chain = Vec::new();
    loop {
        rest = skip_ws(rest);
        match rest.first() {
            Some(b'/') => {
                let len = rest[1..]
                    .iter()
                    .position(|&b| !is_regular(b))
                    .unwrap_or(rest.len() - 1);
                let name = unescape_name(&rest[1..1 + len]);
                // Unknown names make hayro fail the stream, so they cannot expand.
                chain.push(Kind::from_name(&name).unwrap_or(Kind::Image));
                rest = &rest[1 + len..];
                if !array {
                    return Ok(chain);
                }
            }
            Some(b']') if array => return Ok(chain),
            // Indirect reference, garbage, or truncated: cannot verify.
            _ => return Err(()),
        }
    }
}

fn skip_ws(mut s: &[u8]) -> &[u8] {
    loop {
        match s.first() {
            Some(b) if !is_regular(*b) && !matches!(b, b'/' | b'[' | b']' | b'<' | b'(' | b'%') => {
                s = &s[1..]
            }
            Some(b'%') => {
                let n = s
                    .iter()
                    .position(|&b| b == b'\n' || b == b'\r')
                    .unwrap_or(s.len());
                s = &s[n..];
            }
            _ => return s,
        }
    }
}

/// Resolve `#xx` escapes in a name.
fn unescape_name(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'#'
            && let (Some(h), Some(l)) = (raw.get(i + 1).and_then(hex), raw.get(i + 2).and_then(hex))
        {
            out.push(h << 4 | l);
            i += 3;
            continue;
        }
        out.push(raw[i]);
        i += 1;
    }
    out
}

fn hex(b: &u8) -> Option<u8> {
    (*b as char).to_digit(16).map(|d| d as u8)
}

// ─── Decoders ───────────────────────────────────────────────────────

fn flate(data: &[u8], cap: usize) -> Result<Vec<u8>, Limit> {
    use miniz_oxide::inflate::{self, TINFLStatus};
    // Limit + 1 so "exactly cap" is distinguishable from "more than cap".
    let lim = cap.saturating_add(1);
    let res = inflate::decompress_to_vec_zlib_with_limit(data, lim)
        .or_else(|_| inflate::decompress_to_vec_with_limit(data, lim));
    let out = match res {
        Ok(v) => v,
        Err(e) if e.status == TINFLStatus::HasMoreOutput => return Err(over(cap)),
        // Corrupt: keep the zlib attempt's partial output (raw attempt is
        // usually junk when the zlib header was valid).
        Err(_) => match inflate::decompress_to_vec_zlib_with_limit(data, lim) {
            Err(e) if e.status == TINFLStatus::HasMoreOutput => return Err(over(cap)),
            Err(e) => e.output,
            Ok(v) => v,
        },
    };
    if out.len() > cap {
        return Err(over(cap));
    }
    Ok(out)
}

fn run_length(data: &[u8], cap: usize) -> Result<Vec<u8>, Limit> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < data.len() {
        let n = data[i] as usize;
        i += 1;
        match n {
            128 => break,
            0..=127 => {
                let end = (i + n + 1).min(data.len());
                out.extend_from_slice(&data[i..end]);
                i = end;
            }
            _ => {
                let Some(&b) = data.get(i) else { break };
                out.resize(out.len() + 257 - n, b);
                i += 1;
            }
        }
        if out.len() > cap {
            return Err(over(cap));
        }
    }
    Ok(out)
}

/// LZW with EarlyChange = 1 (the PDF default).
fn lzw(data: &[u8], cap: usize) -> Result<Vec<u8>, Limit> {
    const CLEAR: usize = 256;
    const EOD: usize = 257;
    let mut out = Vec::new();
    // Each table entry is (prefix index, last byte, length); strings are rebuilt
    // on demand so the table stays O(4096).
    let mut table: Vec<(usize, u8, usize)> = (0..256).map(|b| (usize::MAX, b as u8, 1)).collect();
    table.push((0, 0, 0));
    table.push((0, 0, 0));
    let mut width = 9;
    let mut prev: Option<usize> = None;
    let (mut buf, mut bits, mut pos) = (0u32, 0u32, 0usize);
    let mut scratch = Vec::new();
    loop {
        while bits < width && pos < data.len() {
            buf = (buf << 8) | data[pos] as u32;
            bits += 8;
            pos += 1;
        }
        if bits < width {
            break;
        }
        let code = ((buf >> (bits - width)) & ((1 << width) - 1)) as usize;
        bits -= width;
        if code == CLEAR {
            table.truncate(258);
            width = 9;
            prev = None;
            continue;
        }
        if code == EOD {
            break;
        }
        let first_of = |table: &Vec<(usize, u8, usize)>, mut c: usize| {
            while table[c].0 != usize::MAX {
                c = table[c].0;
            }
            table[c].1
        };
        let entry = if code < table.len() && code != 256 && code != 257 {
            if let Some(p) = prev
                && table.len() < 4096
            {
                let first = first_of(&table, code);
                table.push((p, first, table[p].2 + 1));
            }
            code
        } else if code == table.len() {
            let Some(p) = prev else { break };
            let first = first_of(&table, p);
            table.push((p, first, table[p].2 + 1));
            code
        } else {
            break;
        };
        // Emit the string for `entry`.
        scratch.clear();
        let mut c = entry;
        loop {
            scratch.push(table[c].1);
            if table[c].0 == usize::MAX {
                break;
            }
            c = table[c].0;
        }
        out.extend(scratch.iter().rev());
        if out.len() > cap {
            return Err(over(cap));
        }
        prev = Some(entry);
        if table.len() + 1 >= (1 << width) && width < 12 {
            width += 1;
        }
    }
    Ok(out)
}

fn ascii_hex(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() / 2);
    let mut hi: Option<u8> = None;
    for b in data {
        if *b == b'>' {
            break;
        }
        let Some(v) = hex(b) else { continue };
        match hi.take() {
            Some(h) => out.push(h << 4 | v),
            None => hi = Some(v),
        }
    }
    if let Some(h) = hi {
        out.push(h << 4);
    }
    out
}

fn ascii85(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() * 4 / 5);
    let mut group = [0u8; 5];
    let mut n = 0;
    let body = data.strip_prefix(b"<~").unwrap_or(data);
    for &b in body {
        match b {
            b'~' => break,
            b'z' if n == 0 => out.extend_from_slice(&[0; 4]),
            b'!'..=b'u' => {
                group[n] = b - b'!';
                n += 1;
                if n == 5 {
                    let v = group.iter().fold(0u64, |a, &d| a * 85 + d as u64);
                    out.extend_from_slice(&(v as u32).to_be_bytes());
                    n = 0;
                }
            }
            _ => {}
        }
    }
    if n > 1 {
        for g in group.iter_mut().skip(n) {
            *g = 84;
        }
        let v = group.iter().fold(0u64, |a, &d| a * 85 + d as u64);
        out.extend_from_slice(&(v as u32).to_be_bytes()[..n - 1]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zlib(data: &[u8]) -> Vec<u8> {
        miniz_oxide::deflate::compress_to_vec_zlib(data, 9)
    }

    #[test]
    fn flate_round_trip_and_cap() {
        let z = zlib(&[b'a'; 10_000]);
        assert_eq!(decode(&[Kind::Flate], &z, 10_000).unwrap().len(), 10_000);
        assert!(decode(&[Kind::Flate], &z, 9_999).is_err());
    }

    #[test]
    fn corrupt_flate_keeps_partial_output() {
        let mut z = zlib(b"hello world, hello world, hello world");
        let n = z.len();
        z.truncate(n - 6);
        assert!(decode(&[Kind::Flate], &z, 1000).is_ok());
    }

    #[test]
    fn run_length_bounded() {
        let bomb: Vec<u8> = std::iter::repeat_n([129u8, b'x'], 1000).flatten().collect();
        assert!(run_length(&bomb, 1000).is_err());
        assert_eq!(
            run_length(&[2, b'a', b'b', b'c', 254, b'z', 128], 100).unwrap(),
            b"abczzz"
        );
    }

    #[test]
    fn lzw_spec_example() {
        // PDF 32000-1 §7.4.4.2 example: "-----A---B".
        let enc = [0x80, 0x0B, 0x60, 0x50, 0x22, 0x0C, 0x0C, 0x85, 0x01];
        assert_eq!(lzw(&enc, 100).unwrap(), b"-----A---B");
        assert!(lzw(&enc, 5).is_err());
    }

    #[test]
    fn ascii_filters() {
        assert_eq!(ascii_hex(b"48 65 6C6C6F>"), b"Hello");
        assert_eq!(ascii_hex(b"7>"), [0x70]);
        assert_eq!(ascii85(b"<~87cURDZ~>"), b"Hello");
    }

    #[test]
    fn filter_chain_parsing() {
        assert_eq!(filter_chain(b"<< /Length 5 >>"), Ok(vec![]));
        assert_eq!(
            filter_chain(b"<</Filter/FlateDecode>>"),
            Ok(vec![Kind::Flate])
        );
        assert_eq!(
            filter_chain(b"<< /Filter [ /A85 /Fl#61teDecode ] >>"),
            Ok(vec![Kind::Ascii85, Kind::Flate])
        );
        assert_eq!(
            filter_chain(b"<< /Filter /DCTDecode >>"),
            Ok(vec![Kind::Image])
        );
        assert_eq!(filter_chain(b"<< /Filter 5 0 R >>"), Err(()));
        assert_eq!(
            filter_chain(b"<< /D << /Filter /AHx >> /Filter /Fl >>"),
            Err(())
        );
    }

    #[test]
    fn prescan_rejects_bomb_and_accepts_small() {
        let mk = |payload: &[u8]| {
            let z = zlib(payload);
            let mut pdf = b"%PDF-1.7\n1 0 obj\n<< /Length ".to_vec();
            pdf.extend(format!("{} /Filter /FlateDecode >>\nstream\n", z.len()).bytes());
            pdf.extend(&z);
            pdf.extend(b"\nendstream\nendobj\n");
            pdf
        };
        assert!(prescan(&mk(&[0; 100]), 1000, 1000).is_ok());
        assert!(matches!(
            prescan(&mk(&[0; 2000]), 1000, 10_000),
            Err(PrescanError::Limit(_))
        ));
        let mut two = mk(&[0; 600]);
        two.extend(mk(&[0; 600]));
        assert!(matches!(
            prescan(&two, 1000, 1000),
            Err(PrescanError::Limit(_))
        ));
    }

    #[test]
    fn prescan_flags_encryption_and_indirect_filters() {
        assert!(matches!(
            prescan(b"trailer << /Encrypt 5 0 R >>", 10, 10),
            Err(PrescanError::Encrypted)
        ));
        assert!(prescan(b"<< /EncryptMetadata false >>", 10, 10).is_ok());
        assert!(matches!(
            prescan(
                b"1 0 obj << /Filter 2 0 R >>\nstream\nxx\nendstream",
                10,
                10
            ),
            Err(PrescanError::Unverifiable)
        ));
        // Words containing "stream" are not stream keywords.
        assert!(prescan(b"(upstream)\n", 10, 10).is_ok());
    }
    #[test]
    fn unfiltered_streams_obey_caps() {
        assert!(matches!(
            prescan(b"1 0 obj << >>\nstream\n123456789\nendstream", 4, 100),
            Err(PrescanError::Limit(_))
        ));
    }

    #[cfg(feature = "pdf-ocr")]
    #[test]
    fn image_codec_cannot_be_used_as_structural_stream() {
        assert!(matches!(
            prescan(
                b"1 0 obj << /Type /ObjStm /Filter /JBIG2Decode >>\nstream\nx\nendstream",
                100,
                100
            ),
            Err(PrescanError::Unverifiable)
        ));
    }

    #[cfg(all(feature = "pdf-ocr", not(target_arch = "wasm32")))]
    #[test]
    fn render_preflight_bounds_images_and_refuses_unbounded_codecs() {
        let stream = |dict: &str, data: &[u8]| {
            let mut bytes = format!("1 0 obj << {dict} >>\nstream\n").into_bytes();
            bytes.extend_from_slice(data);
            bytes.extend_from_slice(b"\nendstream");
            bytes
        };
        let image = "/Type /XObject /Subtype /Image /Width 10 /Height 10";
        assert!(preflight_render(&stream(image, &[255; 100]), 10000, 20000).is_ok());
        for extra in [
            "/Filter /JBIG2Decode",
            "/Filter /CCITTFaxDecode",
            "/DecodeParms << /Columns 999999999 >>",
        ] {
            assert!(matches!(
                preflight_render(&stream(&format!("{image} {extra}"), b"x"), 10000, 20000),
                Err(PrescanError::RenderUnsupported)
            ));
        }
        assert!(matches!(
            preflight_render(
                &stream("/Subtype /Image /Width 999999999 /Height 999999999", b"x"),
                10000,
                20000
            ),
            Err(PrescanError::Limit(_))
        ));
        assert!(matches!(
            preflight_render(
                &stream("/Subtype /Image /Width 10 0 R /Height 10", b"x"),
                10000,
                20000
            ),
            Err(PrescanError::Unverifiable)
        ));
        assert!(matches!(
            preflight_render(
                &stream("", b"BI /W 1 /H 1 /BPC 8 /CS /G ID x EI"),
                10000,
                20000
            ),
            Err(PrescanError::RenderUnsupported)
        ));
    }
    #[test]
    fn jpeg2000_bounds_use_codestream_not_pdf_dictionary() {
        let mut code = b"\xff\x4f\xff\x51\x00\x29\x00\x00".to_vec();
        for n in [100_u32, 200, 0, 0, 100, 200, 0, 0] {
            code.extend(n.to_be_bytes());
        }
        code.extend([0, 1, 7, 1, 1]);
        assert_eq!(jpx_pixels(&code), Some(20000));
        let mut boxed = ((code.len() + 8) as u32).to_be_bytes().to_vec();
        boxed.extend(b"jp2c");
        boxed.extend(&code);
        assert_eq!(jpx_pixels(&boxed), Some(20000));
        code[24..28].copy_from_slice(&u32::MAX.to_be_bytes());
        assert_eq!(jpx_pixels(&code), (u32::MAX as usize).checked_mul(200));
        #[cfg(all(feature = "pdf-ocr", not(target_arch = "wasm32")))]
        {
            let mut stream =
                b"1 0 obj << /Subtype /Image /Width 10 /Height 10 /Filter /JPXDecode >>\nstream\n"
                    .to_vec();
            stream.extend(&code);
            stream.extend(b"\nendstream");
            assert!(matches!(
                preflight_render(&stream, 10000, 20000),
                Err(PrescanError::Limit(_))
            ));
        }
        code[40..42].copy_from_slice(&500_u16.to_be_bytes());
        assert_eq!(jpx_pixels(&code), None);
        assert_eq!(jpx_pixels(b"\x00\x00\x00\x01jp2c"), None);
    }
    #[cfg(all(feature = "pdf-ocr", not(target_arch = "wasm32")))]
    #[test]
    fn sequential_image_working_buffers_are_not_charged_as_retained_bytes() {
        let image = b"1 0 obj << /Subtype /Image /Width 10 /Height 10 /Filter /DCTDecode >>\nstream\nx\nendstream\nendobj\n";
        assert!(preflight_render(&image.repeat(5), 2000, 6500).is_ok());
        assert!(matches!(
            preflight_render(image, 1500, 6500),
            Err(PrescanError::Limit(_))
        ));
    }
    #[test]
    fn image_filter_prefixes_cannot_bypass_decoded_caps() {
        let mut bytes = b"1 0 obj << /Subtype /Image /Width 1 /Height 1 /Filter [/FlateDecode /DCTDecode] >>\nstream\n".to_vec();
        bytes.extend(zlib(&vec![0; 2000]));
        bytes.extend(b"\nendstream");
        assert!(matches!(
            prescan(&bytes, 1000, 10000),
            Err(PrescanError::Limit(_))
        ));
    }

    #[test]
    fn escaped_dictionary_keys_cannot_bypass_stream_guards() {
        let mut bytes =
            b"1 0 obj << /Type /ObjStm /N 1 /First 4 /Fil#74er /FlateDecode >>\nstream\n".to_vec();
        bytes.extend(zlib(&vec![0; 2000]));
        bytes.extend(b"\nendstream");
        assert!(matches!(
            prescan(&bytes, 1000, 10000),
            Err(PrescanError::Limit(_))
        ));
        assert!(matches!(
            prescan(b"trailer << /En#63rypt 5 0 R >>", 1000, 10000),
            Err(PrescanError::Encrypted)
        ));
        assert_eq!(filter_chain(b"<< /Filter /Fl /Fil#74er /AHx >>"), Err(()));
    }

    #[cfg(all(feature = "pdf-ocr", not(target_arch = "wasm32")))]
    #[test]
    fn escaped_image_names_and_dimensions_are_preflighted() {
        let bytes = b"1 0 obj << /Sub#74ype /Ima#67e /W#69dth 999999999 /Height 999999999 >>\nstream\nx\nendstream";
        assert!(matches!(
            preflight_render(bytes, 10000, 20000),
            Err(PrescanError::Limit(_))
        ));
    }
}
