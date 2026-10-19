//! Reading a Google Takeout export on the host side (sources get no file
//! access). Accepts the downloaded `.zip` as is, or an extracted CSV.
//!
//! A Takeout `.zip` can be gigabytes when videos are included, so the
//! archive is never loaded whole: only its central directory and the
//! small CSV entries are read.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use crate::error::{Error, Result};
use crate::youtube::import::parse_takeout_csv;

/// CSV entries larger than this are skipped (and refused as a plain file):
/// a subscriptions list with thousands of channels is well under 1 MiB.
pub const MAX_CSV_BYTES: u64 = 8 * 1024 * 1024;

/// Returns the text of the subscriptions CSV in a Takeout `.zip`, or of a
/// CSV file picked directly.
pub fn subscriptions_csv(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let size = file.metadata()?.len();
    let mut magic = [0u8; 4];
    let n = file.read(&mut magic)?;
    file.seek(SeekFrom::Start(0))?;
    match &magic[..n] {
        [b'P', b'K', 3, 4] | [b'P', b'K', 5, 6] => from_zip(file, size),
        [0x1f, 0x8b, ..] => Err(Error::TakeoutTgz),
        _ => from_csv(file, size),
    }
}

fn from_csv(file: File, size: u64) -> Result<String> {
    if size > MAX_CSV_BYTES {
        return Err(Error::NotATakeoutFile);
    }
    let mut bytes = Vec::new();
    file.take(MAX_CSV_BYTES).read_to_end(&mut bytes)?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    if parse_takeout_csv(&text).channels.is_empty() {
        return Err(Error::NotATakeoutFile);
    }
    Ok(text)
}

fn from_zip(file: File, size: u64) -> Result<String> {
    let mut archive = zip::ZipArchive::new(file).map_err(|_| Error::NotATakeoutFile)?;
    // Takeout names files in the account's language (subscriptions.csv,
    // abonnements.csv, ...), so every small CSV is parsed and the one with
    // the most channels wins. Other YouTube CSVs hold at most the user's
    // own channel; a name hint breaks ties.
    let mut best: Option<(usize, bool, String)> = None;
    for i in 0..archive.len() {
        let Ok(mut entry) = archive.by_index(i) else {
            continue;
        };
        let Ok(name) = entry.name().map(|n| n.to_lowercase()) else {
            continue;
        };
        if !entry.is_file() || !name.ends_with(".csv") || entry.size() > MAX_CSV_BYTES {
            continue;
        }
        let mut bytes = Vec::new();
        if (&mut entry)
            .take(MAX_CSV_BYTES)
            .read_to_end(&mut bytes)
            .is_err()
        {
            continue;
        }
        let text = String::from_utf8_lossy(&bytes).into_owned();
        let count = parse_takeout_csv(&text).channels.len();
        let hinted = ["subscri", "abonne", "suscrip", "iscrizion", "inscri", "abo"]
            .iter()
            .any(|h| name.contains(h));
        if count > 0
            && best
                .as_ref()
                .is_none_or(|(c, h, _)| (count, hinted) > (*c, *h))
        {
            best = Some((count, hinted, text));
        }
    }
    best.map(|(_, _, text)| text)
        .ok_or(Error::TakeoutWithoutSubscriptions {
            size_mb: size / (1024 * 1024),
        })
}
