//! WOFF2 output.
//!
//! There is no WOFF2 encoder on crates.io, so this writes the container
//! directly. Table data is stored **untransformed** (the null transform), which
//! the specification allows: transform version `0` for every table except `glyf`
//! and `loca`, which use version `3` to mean "null transform". The result is a
//! valid WOFF2 that browsers and font tools accept, just larger than a fully
//! transformed file.
//!
//! The reference for the container layout is the W3C WOFF2 specification and
//! fontTools' `ttLib/woff2.py`.

use std::collections::BTreeMap;

use crate::error::{ExportError, Result};

/// The 63 predefined table tags, in specification order. A table whose tag is
/// not in this list sets the flag index to 63 and writes its own 4-byte tag.
const KNOWN_TAGS: [&str; 63] = [
    "cmap", "head", "hhea", "hmtx", "maxp", "name", "OS/2", "post", "cvt ", "fpgm", "glyf",
    "loca", "prep", "CFF ", "VORG", "EBDT", "EBLC", "gasp", "hdmx", "kern", "LTSH", "PCLT",
    "VDMX", "vhea", "vmtx", "BASE", "GDEF", "GPOS", "GSUB", "EBSC", "JSTF", "MATH", "CBDT",
    "CBLC", "COLR", "CPAL", "SVG ", "sbix", "acnt", "avar", "bdat", "bloc", "bsln", "cvar",
    "fdsc", "feat", "fmtx", "fvar", "gvar", "hsty", "just", "lcar", "mort", "morx", "opbd",
    "prop", "trak", "Zapf", "Silf", "Glat", "Gloc", "Feat", "Sill",
];

/// Flag value meaning "the tag is not one of the 63 known tags".
const UNKNOWN_TAG_INDEX: u8 = 63;

/// TrueType outlines, where the null transform is version 3 rather than 0.
fn null_transform_version(tag: &str) -> u8 {
    if tag == "glyf" || tag == "loca" {
        3
    } else {
        0
    }
}

fn known_tag_index(tag: &str) -> u8 {
    KNOWN_TAGS
        .iter()
        .position(|t| *t == tag)
        .map(|i| i as u8)
        .unwrap_or(UNKNOWN_TAG_INDEX)
}

/// One table of the input SFNT.
pub struct Table {
    pub tag: [u8; 4],
    pub data: Vec<u8>,
}

/// Parse the SFNT table directory of a `.ttf` or `.otf`.
pub fn parse_sfnt_tables(font: &[u8]) -> Result<Vec<Table>> {
    if font.len() < 12 {
        return Err(ExportError::MalformedSfnt("file is shorter than an SFNT header".into()));
    }
    let flavor = &font[0..4];
    if flavor != [0x00, 0x01, 0x00, 0x00] && flavor != b"OTTO" && flavor != b"true" {
        return Err(ExportError::MalformedSfnt(format!(
            "unexpected flavor {flavor:?}, expected a TrueType or CFF outline font"
        )));
    }

    let num_tables = u16::from_be_bytes([font[4], font[5]]) as usize;
    let dir_end = 12 + num_tables * 16;
    if font.len() < dir_end {
        return Err(ExportError::MalformedSfnt(format!(
            "table directory claims {num_tables} tables but the file is only {} bytes",
            font.len()
        )));
    }

    let mut tables = Vec::with_capacity(num_tables);
    for i in 0..num_tables {
        let rec = 12 + i * 16;
        let mut tag = [0u8; 4];
        tag.copy_from_slice(&font[rec..rec + 4]);
        let offset = u32::from_be_bytes([
            font[rec + 8],
            font[rec + 9],
            font[rec + 10],
            font[rec + 11],
        ]) as usize;
        let length = u32::from_be_bytes([
            font[rec + 12],
            font[rec + 13],
            font[rec + 14],
            font[rec + 15],
        ]) as usize;
        let end = offset.checked_add(length).ok_or_else(|| {
            ExportError::MalformedSfnt(format!("table {} has an overflowing length", String::from_utf8_lossy(&tag)))
        })?;
        if end > font.len() {
            return Err(ExportError::MalformedSfnt(format!(
                "table {} runs past the end of the file",
                String::from_utf8_lossy(&tag)
            )));
        }
        tables.push(Table { tag, data: font[offset..end].to_vec() });
    }
    Ok(tables)
}

/// Encode a variable-length UIntBase128 value, most significant group first.
///
/// The first byte of each group carries the continuation flag in bit 7.
fn push_base128(out: &mut Vec<u8>, mut value: u32) {
    let mut groups = [0u8; 5];
    let mut n = 0;
    loop {
        groups[n] = (value & 0x7F) as u8;
        value >>= 7;
        n += 1;
        if value == 0 {
            break;
        }
    }
    for i in (0..n).rev() {
        let last = i == 0;
        out.push(if last { groups[i] } else { groups[i] | 0x80 });
    }
}

/// Encode a `.ttf` or `.otf` as WOFF2 using the null transform.
pub fn encode_woff2(font: &[u8]) -> Result<Vec<u8>> {
    let mut tables = parse_sfnt_tables(font)?;
    if tables.is_empty() {
        return Err(ExportError::MalformedSfnt("font has no tables".into()));
    }

    // The specification's table order is alphabetical by tag, and both the
    // legacy OpenType Sanitiser in browsers and fontTools rely on it.
    tables.sort_by_key(|a| a.tag);

    let flavor = &font[0..4];
    let num_tables = tables.len() as u16;

    // Total size the reconstructed SFNT will occupy.
    let total_sfnt_size: u32 = 12
        + u32::try_from(tables.len() * 16).map_err(|_| ExportError::TooLarge("too many tables".into()))?
        + tables.iter().map(|t| pad4(t.data.len() as u32)).sum::<u32>();

    // Table directory.
    let mut directory: Vec<u8> = Vec::new();
    for t in &tables {
        let tag_str = std::str::from_utf8(&t.tag).unwrap_or("");
        let index = known_tag_index(tag_str);
        let transform_version = null_transform_version(tag_str);
        // Bit 7 set means "transformed"; it is clear here because the transform
        // version is the null one. Bits 6-7 hold the version itself.
        let flags = index | (transform_version << 6);
        directory.push(flags);
        if index == UNKNOWN_TAG_INDEX {
            directory.extend_from_slice(&t.tag);
        }
        push_base128(&mut directory, t.data.len() as u32);
        // The null transform means no transformLength field is written.
    }

    // Concatenated table data, in directory order, then brotli-compressed.
    let mut raw = Vec::new();
    for t in &tables {
        raw.extend_from_slice(&t.data);
    }
    let compressed = brotli_compress(&raw);

    let header_size = 48usize;
    let data_offset = header_size + directory.len();
    // The compressed stream is padded to a 4-byte boundary, and the header's
    // length field covers that padding too.
    let total_length = (data_offset + compressed.len()).div_ceil(4) * 4;
    if total_length > u32::MAX as usize {
        return Err(ExportError::TooLarge("WOFF2 file exceeds 4 GiB".into()));
    }

    let mut out: Vec<u8> = Vec::with_capacity(total_length);
    out.extend_from_slice(b"wOF2");
    out.extend_from_slice(flavor);
    out.extend_from_slice(&(total_length as u32).to_be_bytes());
    out.extend_from_slice(&num_tables.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes()); // reserved
    out.extend_from_slice(&total_sfnt_size.to_be_bytes());
    out.extend_from_slice(&(compressed.len() as u32).to_be_bytes());
    out.extend_from_slice(&1u16.to_be_bytes()); // majorVersion
    out.extend_from_slice(&0u16.to_be_bytes()); // minorVersion
    out.extend_from_slice(&0u32.to_be_bytes()); // metaOffset
    out.extend_from_slice(&0u32.to_be_bytes()); // metaLength
    out.extend_from_slice(&0u32.to_be_bytes()); // metaOrigLength
    out.extend_from_slice(&0u32.to_be_bytes()); // privOffset
    out.extend_from_slice(&0u32.to_be_bytes()); // privLength

    out.extend_from_slice(&directory);
    out.extend_from_slice(&compressed);

    // Pad the compressed stream to a 4-byte boundary.
    while out.len() % 4 != 0 {
        out.push(0);
    }
    debug_assert_eq!(out.len(), total_length);
    Ok(out)
}

/// Compress with the parameters Google's woff2 tooling uses for fonts: a window
/// of 22 and quality 11 give the best ratio for font data, which is mostly
/// high-entropy point arrays rather than prose.
///
/// The streaming writer is used rather than the one-shot slice API, which does
/// not terminate when the output slice is pre-sized.
fn brotli_compress(data: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut writer = brotli::CompressorWriter::new(Vec::new(), 4096, 11, 22);
    writer.write_all(data).expect("writing to a Vec cannot fail");
    writer.into_inner()
}

fn pad4(n: u32) -> u32 {
    (n + 3) & !3
}

/// Read back a WOFF2 file's table directory.
///
/// Returns each table's original length together with the offset at which the
/// brotli stream begins. Used by the round-trip tests and by the app's
/// "is this file valid" check.
pub fn read_woff2_directory(data: &[u8]) -> Result<(BTreeMap<String, u32>, usize)> {
    if data.len() < 48 || &data[0..4] != b"wOF2" {
        return Err(ExportError::MalformedSfnt("not a WOFF2 file".into()));
    }
    let num_tables = u16::from_be_bytes([data[12], data[13]]);
    let mut out = BTreeMap::new();
    let mut flags_by_tag: BTreeMap<String, u8> = BTreeMap::new();
    let mut pos = 48usize;
    for _ in 0..num_tables {
        let flags = *data
            .get(pos)
            .ok_or_else(|| ExportError::MalformedSfnt("truncated WOFF2 directory".into()))?;
        pos += 1;
        let index = flags & 0x3F;
        let tag = if index == UNKNOWN_TAG_INDEX {
            let tag = data
                .get(pos..pos + 4)
                .ok_or_else(|| ExportError::MalformedSfnt("truncated WOFF2 tag".into()))?;
            pos += 4;
            std::str::from_utf8(tag).unwrap_or("????").to_string()
        } else {
            KNOWN_TAGS[index as usize].to_string()
        };
        let (orig_length, consumed) = read_base128(&data[pos..])
            .ok_or_else(|| ExportError::MalformedSfnt("truncated WOFF2 length".into()))?;
        pos += consumed;
        let transformed = (flags >> 6) != null_transform_version(&tag);
        if transformed {
            let (_, consumed) = read_base128(&data[pos..]).ok_or_else(|| {
                ExportError::MalformedSfnt("truncated WOFF2 transform length".into())
            })?;
            pos += consumed;
        }
        flags_by_tag.insert(tag.clone(), flags);
        out.insert(tag, orig_length);
    }
    let _ = flags_by_tag;
    Ok((out, pos))
}

/// Table lengths of a WOFF2 file.
pub fn read_woff2_header(data: &[u8]) -> Result<BTreeMap<String, u32>> {
    read_woff2_directory(data).map(|(tables, _)| tables)
}

/// Inverse of [`push_base128`].
fn read_base128(data: &[u8]) -> Option<(u32, usize)> {
    let mut result: u32 = 0;
    for (i, byte) in data.iter().enumerate() {
        if i >= 5 {
            return None;
        }
        result = (result << 7) | (byte & 0x7F) as u32;
        if byte & 0x80 == 0 {
            return Some((result, i + 1));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base128_roundtrip() {
        for value in [0u32, 1, 127, 128, 255, 16_383, 16_384, 100_000, u32::MAX] {
            let mut buf = Vec::new();
            push_base128(&mut buf, value);
            let (decoded, used) = read_base128(&buf).expect("decodes");
            assert_eq!(decoded, value, "value {value}");
            assert_eq!(used, buf.len());
        }
    }

    #[test]
    fn base128_is_smallest_encoding() {
        let mut buf = Vec::new();
        push_base128(&mut buf, 127);
        assert_eq!(buf, vec![0x7F]);
        let mut buf = Vec::new();
        push_base128(&mut buf, 128);
        assert_eq!(buf, vec![0x81, 0x00]);
    }

    #[test]
    fn known_tag_indices() {
        assert_eq!(known_tag_index("cmap"), 0);
        assert_eq!(known_tag_index("glyf"), 10);
        assert_eq!(known_tag_index("GSUB"), 28);
        assert_eq!(known_tag_index("zzzz"), UNKNOWN_TAG_INDEX);
    }

    #[test]
    fn null_transform_versions() {
        assert_eq!(null_transform_version("head"), 0);
        assert_eq!(null_transform_version("glyf"), 3);
        assert_eq!(null_transform_version("loca"), 3);
    }

    #[test]
    fn rejects_non_sfnt() {
        let err = encode_woff2(b"not a font at all").unwrap_err();
        assert!(matches!(err, ExportError::MalformedSfnt(_)));
    }

    #[test]
    fn rejects_truncated_header() {
        assert!(parse_sfnt_tables(&[0, 1, 0, 0, 0, 5]).is_err());
    }

    /// A synthetic SFNT with two tables, enough to exercise the container.
    fn tiny_sfnt() -> Vec<u8> {
        let glyf = vec![0u8; 40];
        let cmap = (0u8..60).collect::<Vec<u8>>();
        let mut out: Vec<u8> = Vec::new();
        out.extend_from_slice(&[0x00, 0x01, 0x00, 0x00]); // TrueType
        out.extend_from_slice(&2u16.to_be_bytes()); // numTables
        out.extend_from_slice(&32u16.to_be_bytes()); // searchRange
        out.extend_from_slice(&1u16.to_be_bytes()); // entrySelector
        out.extend_from_slice(&0u16.to_be_bytes()); // rangeShift

        // Records must be sorted by tag: cmap then glyf.
        let mut record = |tag: &[u8; 4], offset: u32, length: u32, sum: u32| {
            out.extend_from_slice(tag);
            out.extend_from_slice(&sum.to_be_bytes());
            out.extend_from_slice(&offset.to_be_bytes());
            out.extend_from_slice(&length.to_be_bytes());
        };
        record(b"cmap", 44, cmap.len() as u32, 0);
        // Table data starts right after the 12-byte header and two 16-byte
        // records, and each table begins on a 4-byte boundary.
        record(b"glyf", 104, glyf.len() as u32, 0);
        out.extend_from_slice(&cmap);
        out.extend_from_slice(&glyf);
        out
    }

    #[test]
    fn roundtrips_through_decompression() {
        let sfnt = tiny_sfnt();
        let woff2 = encode_woff2(&sfnt).unwrap();

        assert_eq!(&woff2[0..4], b"wOF2");
        assert_eq!(&woff2[4..8], &[0x00, 0x01, 0x00, 0x00], "flavour is preserved");
        let declared_len = u32::from_be_bytes([woff2[8], woff2[9], woff2[10], woff2[11]]) as usize;
        assert_eq!(
            declared_len,
            woff2.len(),
            "the header length field must match the file size"
        );
        assert_eq!(woff2.len() % 4, 0, "file is padded to a 4-byte boundary");

        // Decompress the stream and check the tables come back byte for byte.
        let (tables, data_offset) = read_woff2_directory(&woff2).unwrap();
        assert_eq!(tables.len(), 2);
        assert_eq!(tables["cmap"], 60);
        assert_eq!(tables["glyf"], 40);

        let mut raw: Vec<u8> = Vec::new();
        let mut input = &woff2[data_offset..];
        brotli::BrotliDecompress(&mut input, &mut raw).expect("brotli stream is valid");
        let expected: Vec<u8> = (0u8..60).chain(vec![0u8; 40]).collect();
        assert_eq!(raw, expected, "table data survives the round trip");
    }

    #[test]
    fn compression_actually_shrinks() {
        let sfnt = tiny_sfnt();
        let woff2 = encode_woff2(&sfnt).unwrap();
        assert!(
            woff2.len() < sfnt.len(),
            "compressed {} into {} bytes",
            sfnt.len(),
            woff2.len()
        );
    }

    #[test]
    fn glyf_and_loca_use_transform_version_three() {
        // A font with glyf must not claim the non-glyf null transform (0),
        // or a decoder will try to apply the glyf transform to untransformed data.
        let sfnt = tiny_sfnt();
        let woff2 = encode_woff2(&sfnt).unwrap();
        let flags = find_directory_flags(&woff2, "glyf").expect("glyf entry");
        assert_eq!(flags & 0x3F, 10, "glyf tag index");
        // Bits 6-7 are the transform version; 3 is the null transform for glyf.
        // A decoder decides "is this transformed" by comparing against the null
        // version for the table, so bit 7 being set here is correct.
        assert_eq!(flags >> 6, 3, "glyf null transform version");

        let cmap_flags = find_directory_flags(&woff2, "cmap").expect("cmap entry");
        assert_eq!(cmap_flags >> 6, 0, "cmap null transform version");
        assert_eq!(cmap_flags & 0xC0, 0, "a non-glyf table uses version 0");
    }

    /// Flags byte of one table's directory entry, found by walking the entries.
    fn find_directory_flags(data: &[u8], want: &str) -> Option<u8> {
        let (_, _) = read_woff2_directory(data).ok()?;
        let num_tables = u16::from_be_bytes([data[12], data[13]]) as usize;
        let mut pos = 48usize;
        for _ in 0..num_tables {
            let flags = *data.get(pos)?;
            pos += 1;
            let index = flags & 0x3F;
            let tag = if index == UNKNOWN_TAG_INDEX {
                let tag = std::str::from_utf8(data.get(pos..pos + 4)?).ok()?.to_string();
                pos += 4;
                tag
            } else {
                KNOWN_TAGS[index as usize].to_string()
            };
            let (len, used) = read_base128(&data[pos..])?;
            pos += used;
            if (flags >> 6) != null_transform_version(&tag) {
                pos += read_base128(&data[pos..])?.1;
            }
            if tag == want {
                return Some(flags);
            }
            let _ = len;
        }
        None
    }
}