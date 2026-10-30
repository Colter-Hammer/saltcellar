//! Taking `db.sqlite` and `metadata.json` out of the export zip.

use std::collections::BTreeSet;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::Path;

use crate::{DB_FILE, METADATA_FILE, ReadError};

/// The most `db.sqlite` may unpack to: far more than tens of thousands of transactions need, and
/// a stop for a zip that inflates without end *(agent)*.
const MAX_DB_BYTES: u64 = 1 << 30;
/// The most `metadata.json` may unpack to *(agent)*.
const MAX_METADATA_BYTES: u64 = 1 << 20;

/// Copies the export's `db.sqlite` and `metadata.json` out of the zip at `zip` into the folder
/// `into`, which must exist. Like Actual's own importer, it takes them from the zip's root, or
/// from inside one folder that holds both.
///
/// # Errors
/// [`ReadError::NotAnExport`] if `zip` isn't a zip or has no such pair, [`ReadError::TooLarge`]
/// if either unpacks to more than the app reads, [`ReadError::Io`] if `into` can't be written.
pub fn unpack(zip: &Path, into: &Path) -> Result<(), ReadError> {
    let mut archive = zip::ZipArchive::new(File::open(zip)?).map_err(|_| ReadError::NotAnExport)?;
    let names = archive
        .file_names()
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| ReadError::NotAnExport)?
        .into_iter()
        .map(std::borrow::Cow::into_owned)
        .collect::<Vec<String>>();
    let folder = locate(names.iter().map(String::as_str)).ok_or(ReadError::NotAnExport)?;
    for (name, limit) in [(DB_FILE, MAX_DB_BYTES), (METADATA_FILE, MAX_METADATA_BYTES)] {
        let mut entry = archive
            .by_name(&format!("{folder}{name}"))
            .map_err(|_| ReadError::NotAnExport)?;
        if !entry.is_file() {
            return Err(ReadError::NotAnExport);
        }
        copy(&mut entry, &mut File::create(into.join(name))?, limit)?;
    }
    Ok(())
}

/// The folder in the zip that holds both files: `""` for the root, or e.g. `"My-Budget/"`.
/// The root wins; otherwise exactly one folder may hold the pair.
fn locate<'a>(names: impl Iterator<Item = &'a str>) -> Option<String> {
    let names: BTreeSet<&str> = names.collect();
    let folders: Vec<&str> = names
        .iter()
        .filter_map(|name| name.strip_suffix(DB_FILE))
        .filter(|folder| {
            folder.is_empty()
                || folder
                    .find('/')
                    .is_some_and(|slash| slash + 1 == folder.len())
        })
        .filter(|folder| names.contains(format!("{folder}{METADATA_FILE}").as_str()))
        .collect();
    if folders.contains(&"") {
        return Some(String::new());
    }
    match folders.as_slice() {
        [folder] => Some((*folder).to_owned()),
        _ => None,
    }
}

/// Copies `from` into `to`, refusing more than `limit` bytes. A failed read is a damaged zip, so
/// not an export; a failed write is the server's.
fn copy(from: &mut impl Read, to: &mut impl Write, limit: u64) -> Result<(), ReadError> {
    let mut buffer = vec![0; 64 * 1024];
    let mut total: u64 = 0;
    loop {
        let read = match from.read(&mut buffer) {
            Ok(0) => return Ok(to.flush()?),
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return Err(ReadError::NotAnExport),
        };
        total += read as u64;
        if total > limit {
            return Err(ReadError::TooLarge);
        }
        to.write_all(buffer.get(..read).unwrap_or_default())?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn located(names: &[&str]) -> Option<String> {
        locate(names.iter().copied())
    }

    #[test]
    fn finds_the_pair_at_the_root() {
        assert_eq!(
            located(&["db.sqlite", "metadata.json"]),
            Some(String::new())
        );
        assert_eq!(
            located(&[
                "metadata.json",
                "db.sqlite",
                "Budget/db.sqlite",
                "Budget/metadata.json"
            ]),
            Some(String::new())
        );
    }

    #[test]
    fn finds_the_pair_in_one_shared_folder() {
        assert_eq!(
            located(&["Budget/", "Budget/db.sqlite", "Budget/metadata.json"]),
            Some("Budget/".to_owned())
        );
        assert_eq!(
            located(&[
                "Budget/db.sqlite",
                "Budget/metadata.json",
                "__MACOSX/Budget/._db.sqlite",
                "Other/db.sqlite",
            ]),
            Some("Budget/".to_owned())
        );
    }

    #[test]
    fn refuses_anything_else() {
        for names in [
            &[][..],
            &["readme.txt"],
            &["db.sqlite"],
            &["metadata.json"],
            &["A/db.sqlite", "B/metadata.json"],
            &[
                "A/db.sqlite",
                "A/metadata.json",
                "B/db.sqlite",
                "B/metadata.json",
            ],
            &["A/B/db.sqlite", "A/B/metadata.json"],
            &["old-db.sqlite", "old-metadata.json"],
        ] {
            assert_eq!(located(names), None, "{names:?}");
        }
    }

    #[test]
    fn copies_up_to_the_limit_only() {
        let mut out = Vec::new();
        copy(&mut &b"12345"[..], &mut out, 5).unwrap();
        assert_eq!(out, b"12345");
        let error = copy(&mut &b"123456"[..], &mut Vec::new(), 5).unwrap_err();
        assert!(matches!(error, ReadError::TooLarge), "{error}");
    }

    #[test]
    fn a_failed_read_is_not_an_export() {
        struct Broken;
        impl Read for Broken {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::new(io::ErrorKind::InvalidData, "bad CRC"))
            }
        }
        let error = copy(&mut Broken, &mut Vec::new(), 5).unwrap_err();
        assert!(matches!(error, ReadError::NotAnExport), "{error}");
    }

    #[test]
    fn a_file_that_isnt_a_zip_is_not_an_export() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("export.zip");
        std::fs::write(&path, b"just some text").unwrap();
        let error = unpack(&path, dir.path()).unwrap_err();
        assert!(matches!(error, ReadError::NotAnExport), "{error}");
    }
}
