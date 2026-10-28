//! Documents made in code for tests.

use std::io::Write;
use std::path::{Path, PathBuf};

use zip::write::SimpleFileOptions;
use zip::CompressionMethod;

fn write_zip(dir: &Path, name: &str, parts: &[(&str, &[u8])], method: CompressionMethod) -> PathBuf {
    let path = dir.join(name);
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    for (part, bytes) in parts {
        zip.start_file(*part, SimpleFileOptions::default().compression_method(method)).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap();
    path
}

/// A zip with deflated parts.
pub(crate) fn zip_file(dir: &Path, name: &str, parts: &[(&str, &[u8])]) -> PathBuf {
    write_zip(dir, name, parts, CompressionMethod::Deflated)
}

/// A zip with stored (uncompressed) parts.
pub(crate) fn zip_stored(dir: &Path, name: &str, parts: &[(&str, &[u8])]) -> PathBuf {
    write_zip(dir, name, parts, CompressionMethod::Stored)
}

/// Overwrites the first entry's method or uncompressed size in both its local and central header.
pub(crate) fn patch_first_entry(path: &Path, method: Option<u16>, size: Option<u32>) {
    let mut bytes = std::fs::read(path).unwrap();
    let local = find(&bytes, b"PK\x03\x04");
    let central = find(&bytes, b"PK\x01\x02");
    if let Some(m) = method {
        bytes[local + 8..local + 10].copy_from_slice(&m.to_le_bytes());
        bytes[central + 10..central + 12].copy_from_slice(&m.to_le_bytes());
    }
    if let Some(s) = size {
        bytes[local + 22..local + 26].copy_from_slice(&s.to_le_bytes());
        bytes[central + 24..central + 28].copy_from_slice(&s.to_le_bytes());
    }
    std::fs::write(path, bytes).unwrap();
}

fn find(hay: &[u8], needle: &[u8]) -> usize {
    hay.windows(needle.len()).position(|w| w == needle).unwrap()
}
