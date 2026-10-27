//! Bounded ZIP access shared by the DOCX and spreadsheet parsers.
//!
//! Decision: declared sizes in ZIP headers are attacker-controlled, so limits are
//! enforced on bytes actually produced by the decompressor (`Read::take`), never on
//! header fields. [`ZipGuard::validate_all`] decompresses every entry into a sink
//! before third-party code (calamine) touches the archive, because that code reads
//! entries without limits and may follow arbitrary relationship targets.

use std::io::{Cursor, Read};

use zip::ZipArchive;
use zip::result::ZipError;

use crate::config::MarkkitBuilder;

/// Archive failure, split so callers can map limit violations to
/// [`crate::Error::ResourceLimit`] and everything else to a format error.
#[derive(Debug)]
pub(crate) enum ArchiveError {
    Limit(String),
    Invalid(String),
}

impl From<ZipError> for ArchiveError {
    fn from(e: ZipError) -> Self {
        ArchiveError::Invalid(format!("invalid ZIP container: {e}"))
    }
}

pub(crate) struct ZipGuard<'a> {
    pub archive: ZipArchive<Cursor<&'a [u8]>>,
    // ZIP 9 indexes raw filename bytes. Keep decoded names in archive order for
    // case-insensitive OPC lookup, including legacy CP437 names.
    names: Vec<String>,
    max_entry: u64,
    max_total: u64,
    total: u64,
}

impl<'a> ZipGuard<'a> {
    /// Open the archive and check the entry count.
    pub fn open(bytes: &'a [u8], config: &MarkkitBuilder) -> Result<Self, ArchiveError> {
        let archive = ZipArchive::new(Cursor::new(bytes))?;
        if archive.len() > config.limits.max_archive_entries {
            return Err(ArchiveError::Limit(format!(
                "archive has {} entries, limit is {}",
                archive.len(),
                config.limits.max_archive_entries
            )));
        }
        let names = archive
            .file_names()
            .map(|name| name.map(|name| name.into_owned()))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            archive,
            names,
            max_entry: config.limits.max_archive_entry_size,
            max_total: config.limits.max_archive_total_size,
            total: 0,
        })
    }

    /// Names of all entries.
    #[cfg(feature = "spreadsheet")]
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.names.iter().map(String::as_str)
    }

    /// Decompress every entry into a sink, enforcing per-entry and total limits.
    #[cfg(feature = "spreadsheet")]
    pub fn validate_all(&mut self) -> Result<(), ArchiveError> {
        for i in 0..self.archive.len() {
            let n = {
                let mut entry = self.archive.by_index(i)?;
                if entry.is_dir() {
                    continue;
                }
                let name = entry.name()?.into_owned();
                copy_capped(&mut entry, self.max_entry, &name, &mut std::io::sink())?
            };
            self.add_total(n)?;
        }
        Ok(())
    }

    /// Read one entry fully (bounded). `Ok(None)` when it does not exist.
    pub fn read(&mut self, name: &str) -> Result<Option<Vec<u8>>, ArchiveError> {
        let name = name.trim_start_matches('/');
        let index = match self.archive.index_for_name(name) {
            Some(i) => i,
            None => {
                // Part names are case-insensitive in OPC.
                match self.names.iter().position(|n| n.eq_ignore_ascii_case(name)) {
                    Some(i) => i,
                    None => return Ok(None),
                }
            }
        };
        let mut buf = Vec::new();
        let n = {
            let mut entry = self.archive.by_index(index)?;
            copy_capped(&mut entry, self.max_entry, name, &mut buf)?
        };
        self.add_total(n)?;
        Ok(Some(buf))
    }

    fn add_total(&mut self, n: u64) -> Result<(), ArchiveError> {
        self.total = self.total.saturating_add(n);
        if self.total > self.max_total {
            return Err(ArchiveError::Limit(format!(
                "archive decompresses to more than {} bytes",
                self.max_total
            )));
        }
        Ok(())
    }
}

fn copy_capped<R: Read, W: std::io::Write>(
    reader: &mut R,
    cap: u64,
    name: &str,
    out: &mut W,
) -> Result<u64, ArchiveError> {
    let n = std::io::copy(&mut reader.take(cap.saturating_add(1)), out)
        .map_err(|e| ArchiveError::Invalid(format!("cannot read entry '{name}': {e}")))?;
    if n > cap {
        return Err(ArchiveError::Limit(format!(
            "entry '{name}' decompresses to more than {cap} bytes"
        )));
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    fn zip_with_name(name: &str) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .start_file(name, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"content").unwrap();
        writer.finish().unwrap().into_inner()
    }

    #[test]
    fn unicode_names_keep_case_insensitive_opc_lookup() {
        let bytes = zip_with_name("café/Document.xml");
        let mut guard = ZipGuard::open(&bytes, &MarkkitBuilder::default()).unwrap();
        assert_eq!(
            guard.read("café/document.XML").unwrap(),
            Some(b"content".to_vec())
        );
    }

    #[test]
    fn legacy_cp437_names_are_read_by_decoded_name() {
        let mut bytes = zip_with_name("e.xml");
        for i in 0..bytes.len() - 4 {
            if bytes[i..i + 5] == *b"e.xml" {
                bytes[i] = 0x82;
            }
        }
        let mut guard = ZipGuard::open(&bytes, &MarkkitBuilder::default()).unwrap();
        assert_eq!(guard.read("é.xml").unwrap(), Some(b"content".to_vec()));
    }
}
