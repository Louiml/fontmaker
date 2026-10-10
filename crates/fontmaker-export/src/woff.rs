//! WOFF 1.0 output.
//!
//! The container is straightforward: a header, one directory entry per table,
//! then each table zlib-compressed and stored back to back. Unlike WOFF2 there
//! are no table transforms, so this is a faithful wrapper of the TrueType file.

use crate::error::{ExportError, Result};
use crate::woff2::parse_sfnt_tables;

const WOFF_SIGNATURE: u32 = 0x774F_4646; // 'wOFF'
const WOFF2_SIGNATURE: u32 = 0x774F_4632; // 'wOF2'

/// Encode a `.ttf` or `.otf` as WOFF 1.0.
pub fn encode_woff(font: &[u8]) -> Result<Vec<u8>> {
    let mut tables = parse_sfnt_tables(font)?;
    tables.sort_by_key(|a| a.tag);

    let flavor = u32::from_be_bytes([font[0], font[1], font[2], font[3]]);

    // The WOFF directory must be sorted by tag, and the sfnt checksum
    // adjustment of the head table has to be recomputed for the new layout.
    // The WOFF directory is 44 bytes plus 20 per table.
    let directory_size = 44 + tables.len() * 20;

    // One entry per table: (checksum of the original data, tag, offset,
    // stored length, original length). `original length` is the *uncompressed*
    // size, which is what lets a reader inflate the table back to the right size.
    let mut entries: Vec<(u32, [u8; 4], u32, u32, u32)> = Vec::with_capacity(tables.len());
    let mut body: Vec<u8> = Vec::new();

    for t in &tables {
        let compressed = zlib_compress(&t.data);
        let original_length = t.data.len() as u32;
        // Storing a table uncompressed is smaller when deflate cannot help.
        let (data, stored_length) = if compressed.len() < t.data.len() {
            let len = compressed.len() as u32;
            (compressed, len)
        } else {
            (t.data.clone(), t.data.len() as u32)
        };

        // The offset in a WOFF directory is measured from the start of the
        // file, not from the start of the table data.
        let offset = (directory_size + body.len()) as u32;
        body.extend_from_slice(&data);
        while body.len() % 4 != 0 {
            body.push(0);
        }
        entries.push((
            checksum(&t.data),
            t.tag,
            offset,
            stored_length,
            original_length,
        ));
    }

    let total_sfnt_size: u32 = 12
        + u32::try_from(tables.len() * 16).map_err(|_| ExportError::TooLarge("too many tables".into()))?
        + entries.iter().map(|e| (e.4 + 3) & !3).sum::<u32>();

    let mut out: Vec<u8> = Vec::with_capacity(directory_size + body.len());
    out.extend_from_slice(&WOFF_SIGNATURE.to_be_bytes());
    out.extend_from_slice(&flavor.to_be_bytes());
    // WOFF stores the length in the header, so it has to be computed up front.
    let total_length = (directory_size + body.len()) as u32;
    out.extend_from_slice(&total_length.to_be_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes()); // reserved
    out.extend_from_slice(&total_sfnt_size.to_be_bytes());
    out.extend_from_slice(&1u16.to_be_bytes()); // majorVersion
    out.extend_from_slice(&0u16.to_be_bytes()); // minorVersion
    // metaOffset, metaLength, metaOrigLength, privOffset, privLength: none.
    for _ in 0..5 {
        out.extend_from_slice(&0u32.to_be_bytes());
    }

    for (sum, tag, offset, stored_length, original_length) in &entries {
        out.extend_from_slice(tag);
        out.extend_from_slice(&offset.to_be_bytes());
        out.extend_from_slice(&stored_length.to_be_bytes());
        out.extend_from_slice(&original_length.to_be_bytes());
        out.extend_from_slice(&sum.to_be_bytes());
    }

    out.extend_from_slice(&body);
    Ok(out)
}

/// SFNT checksum: sum of big-endian u32 words over the padded table.
fn checksum(data: &[u8]) -> u32 {
    let mut sum: u32 = 0;
    let mut chunks = data.chunks_exact(4);
    for c in &mut chunks {
        sum = sum.wrapping_add(u32::from_be_bytes([c[0], c[1], c[2], c[3]]));
    }
    let rest = chunks.remainder();
    if !rest.is_empty() {
        let mut buf = [0u8; 4];
        buf[..rest.len()].copy_from_slice(rest);
        sum = sum.wrapping_add(u32::from_be_bytes(buf));
    }
    sum
}

/// zlib stream, which is what a WOFF table uses.
fn zlib_compress(data: &[u8]) -> Vec<u8> {
    miniz_oxide::deflate::compress_to_vec_zlib(data, 9)
}

/// Recognise a WOFF or WOFF2 signature.
pub fn flavor_of(data: &[u8]) -> Option<&'static str> {
    if data.len() < 4 {
        return None;
    }
    match u32::from_be_bytes([data[0], data[1], data[2], data[3]]) {
        WOFF_SIGNATURE => Some("woff"),
        WOFF2_SIGNATURE => Some("woff2"),
        0x0001_0000 => Some("ttf"),
        0x4F54_544F => Some("otf"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum_matches_sfnt_definition() {
        // "abcd" padded to 4 bytes is a single word.
        assert_eq!(checksum(b"abcd"), 0x6162_6364);
        // Padding a partial word out with zeroes.
        assert_eq!(checksum(b"abc"), 0x6162_6300);
    }

    #[test]
    fn zlib_stream_has_a_valid_header() {
        let out = zlib_compress(b"hello hello hello");
        // CMF byte: deflate with a 32K window.
        assert_eq!(out[0] & 0x0F, 8);
        let flg = out[1];
        assert_eq!(u16::from_be_bytes([out[0], flg]) % 31, 0, "header checksum");
    }

    #[test]
    fn zlib_roundtrips_through_inflate() {
        let original = vec![7u8; 4096];
        let compressed = zlib_compress(&original);
        assert!(compressed.len() < original.len());
        let back = miniz_oxide::inflate::decompress_to_vec_zlib(&compressed).unwrap();
        assert_eq!(back, original);
    }

    #[test]
    fn flavors_are_recognised() {
        assert_eq!(flavor_of(&[0x77, 0x4F, 0x46, 0x46]), Some("woff"));
        assert_eq!(flavor_of(&[0x77, 0x4F, 0x46, 0x32]), Some("woff2"));
        assert_eq!(flavor_of(&[0x00, 0x01, 0x00, 0x00]), Some("ttf"));
        assert_eq!(flavor_of(&[1, 2, 3]), None);
    }
}