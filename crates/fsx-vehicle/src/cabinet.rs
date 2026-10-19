//! Bounded, read-only single-cabinet resources. No extraction or executable loading.
use miniz_oxide::inflate::{
    core::{
        decompress, inflate_flags::TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF, DecompressorOxide,
    },
    TINFLStatus,
};
use std::{collections::BTreeSet, fs::File, io::Read, ops::Range, path::Path};

type Result<T> = std::result::Result<T, String>;
const MAX_BYTES: usize = 64 * 1024 * 1024;
const MAX_ENTRIES: usize = 8192;

#[derive(Debug)]
pub struct Entry {
    pub name: String,
    key: String,
    folder: usize,
    range: Range<usize>,
}
impl Entry {
    pub fn size(&self) -> usize {
        self.range.len()
    }
}

#[derive(Debug)]
pub struct Cabinet {
    entries: Vec<Entry>,
    folders: Vec<Vec<u8>>,
}

fn bytes(b: &[u8], start: usize, len: usize) -> Result<&[u8]> {
    b.get(start..start.checked_add(len).ok_or("CAB offset overflow")?)
        .ok_or_else(|| "Truncated CAB data".into())
}
fn u16_at(b: &[u8], start: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(bytes(b, start, 2)?.try_into().unwrap()))
}
fn u32_at(b: &[u8], start: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(bytes(b, start, 4)?.try_into().unwrap()))
}

/// Canonical relative resource key. Reject aliases and filesystem escape syntax.
pub fn resource_key(name: &str) -> Result<String> {
    if name.is_empty()
        || name.len() > 1024
        || name
            .chars()
            .any(|c| c.is_control() || ":*?\"<>|".contains(c))
    {
        return Err("Invalid CAB resource name".into());
    }
    let name = name.replace('\\', "/");
    if name
        .split('/')
        .any(|p| p.is_empty() || p == "." || p == ".." || p.ends_with([' ', '.']))
    {
        return Err("Unsafe or ambiguous CAB resource path".into());
    }
    Ok(name.to_lowercase())
}

// MS-CAB remainder bytes use the opposite order to complete little-endian words.
fn checksum(data: &[u8]) -> u32 {
    let mut chunks = data.chunks_exact(4);
    let result = chunks
        .by_ref()
        .fold(0, |v, c| v ^ u32::from_le_bytes(c.try_into().unwrap()));
    result
        ^ chunks
            .remainder()
            .iter()
            .fold(0, |v, &b| (v << 8) | u32::from(b))
}

impl Cabinet {
    pub fn load(path: &Path, windows_1252: bool) -> Result<Self> {
        let mut file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        if file.metadata().map_err(|e| e.to_string())?.len() > MAX_BYTES as u64 {
            return Err("CAB file exceeds 64 MiB".into());
        }
        let mut data = Vec::new();
        file.by_ref()
            .take(MAX_BYTES as u64 + 1)
            .read_to_end(&mut data)
            .map_err(|e| e.to_string())?;
        Self::parse(&data, windows_1252)
    }

    pub fn parse(data: &[u8], windows_1252: bool) -> Result<Self> {
        if data.len() > MAX_BYTES || bytes(data, 0, 4)? != b"MSCF" || bytes(data, 24, 2)? != [3, 1]
        {
            return Err("Invalid CAB signature, size or version".into());
        }
        // Signed cabinets may append a certificate after cbCabinet. No offsets
        // used below can reach it; this decoder makes no signature-trust claim.
        let data = bytes(data, 0, u32_at(data, 8)? as usize)?;
        for offset in [4, 12, 20] {
            if u32_at(data, offset)? != 0 {
                return Err("Nonzero CAB reserved header".into());
            }
        }
        let flags = u16_at(data, 30)?;
        if flags & !4 != 0 || u16_at(data, 34)? != 0 {
            return Err("Split, chained or unknown CAB flags are unsupported".into());
        }
        let folder_count = u16_at(data, 26)? as usize;
        let file_count = u16_at(data, 28)? as usize;
        if folder_count == 0 || folder_count > MAX_ENTRIES || file_count > MAX_ENTRIES {
            return Err("CAB table count exceeds bounds".into());
        }
        let (mut cursor, folder_reserve, block_reserve) = if flags & 4 != 0 {
            let n = u16_at(data, 36)? as usize;
            let extra = bytes(data, 38, 2)?;
            bytes(data, 40, n)?;
            (40 + n, extra[0] as usize, extra[1] as usize)
        } else {
            (36, 0, 0)
        };
        let mut folders = Vec::new();
        for _ in 0..folder_count {
            bytes(data, cursor, 8 + folder_reserve)?;
            let compression = u16_at(data, cursor + 6)?;
            if compression > 1 {
                return Err(format!("Unsupported CAB compression {compression:#x}"));
            }
            folders.push((
                u32_at(data, cursor)? as usize,
                u16_at(data, cursor + 4)? as usize,
                compression,
            ));
            cursor += 8 + folder_reserve;
        }
        let file_table = u32_at(data, 16)? as usize;
        if file_table < cursor {
            return Err("CAB file table overlaps header".into());
        }
        cursor = file_table;
        let mut entries = Vec::new();
        let mut names = BTreeSet::new();
        for _ in 0..file_count {
            let size = u32_at(data, cursor)? as usize;
            let start = u32_at(data, cursor + 4)? as usize;
            let folder = u16_at(data, cursor + 8)? as usize;
            let attributes = u16_at(data, cursor + 14)?;
            if folder >= folder_count
                || size > MAX_BYTES
                || start > MAX_BYTES
                || start + size > MAX_BYTES
            {
                return Err("CAB file range or folder is unsupported".into());
            }
            cursor += 16;
            let tail = bytes(data, cursor, data.len().saturating_sub(cursor))?;
            let end = tail
                .iter()
                .take(1025)
                .position(|&b| b == 0)
                .ok_or("Invalid CAB filename terminator")?;
            let raw = &tail[..end];
            let name = if attributes & 0x80 != 0 {
                std::str::from_utf8(raw)
                    .map_err(|_| "Invalid UTF-8 CAB name")?
                    .to_owned()
            } else {
                fsx_cfg::decode(raw, windows_1252)?
            };
            let key = resource_key(&name)?;
            if !names.insert(key.clone()) {
                return Err("Duplicate case-insensitive CAB resource".into());
            }
            entries.push(Entry {
                name,
                key,
                folder,
                range: start..start + size,
            });
            cursor += end + 1;
        }
        let metadata_end = cursor;
        let mut ranges = Vec::new();
        let mut decoded = Vec::new();
        let mut total = 0usize;
        for (start, count, compression) in folders {
            if start < metadata_end || count == 0 {
                return Err("CAB data overlaps metadata or is empty".into());
            }
            cursor = start;
            let mut output = Vec::new();
            for _ in 0..count {
                let expected_checksum = u32_at(data, cursor)?;
                let packed = u16_at(data, cursor + 4)? as usize;
                let unpacked = u16_at(data, cursor + 6)? as usize;
                if packed == 0 || unpacked == 0 || unpacked > 32768 || packed > 38912 {
                    return Err("Invalid or split CAB block size".into());
                }
                let header = bytes(data, cursor + 4, 4 + block_reserve)?;
                cursor += 8 + block_reserve;
                let payload = bytes(data, cursor, packed)?;
                if expected_checksum != 0
                    && expected_checksum != checksum(payload) ^ checksum(header)
                {
                    return Err("CAB block checksum mismatch".into());
                }
                total += unpacked;
                if total > MAX_BYTES {
                    return Err("Decoded CAB exceeds 64 MiB".into());
                }
                let pos = output.len();
                // One extra byte distinguishes a full valid output from a stream that
                // would exceed the declared size. Prior folder data is the dictionary.
                output.resize(pos + unpacked + 1, 0);
                if compression == 0 {
                    if packed != unpacked {
                        return Err("Stored CAB block size mismatch".into());
                    }
                    output[pos..pos + unpacked].copy_from_slice(payload);
                } else {
                    if !payload.starts_with(b"CK") {
                        return Err("Missing MSZIP CK signature".into());
                    }
                    let (status, consumed, written) = decompress(
                        &mut DecompressorOxide::new(),
                        &payload[2..],
                        &mut output,
                        pos,
                        TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF,
                    );
                    if status != TINFLStatus::Done || consumed != packed - 2 || written != unpacked
                    {
                        return Err("Invalid MSZIP stream or declared output size".into());
                    }
                }
                output.truncate(pos + unpacked);
                cursor += packed;
            }
            ranges.push(start..cursor);
            decoded.push(output);
        }
        ranges.sort_by_key(|r| r.start);
        if ranges.windows(2).any(|r| r[0].end > r[1].start) {
            return Err("Overlapping CAB folder data".into());
        }
        for entry in &entries {
            if entry.range.end > decoded[entry.folder].len() {
                return Err("CAB file exceeds decoded folder".into());
            }
        }
        Ok(Self {
            entries,
            folders: decoded,
        })
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn resource(&self, name: &str) -> Result<&[u8]> {
        let key = resource_key(name)?;
        let entry = self
            .entries
            .iter()
            .find(|e| e.key == key)
            .ok_or_else(|| format!("CAB resource not found: {name}"))?;
        Ok(&self.folders[entry.folder][entry.range.clone()])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str, blocks: &[(&[u8], usize)], compression: u16) -> Vec<u8> {
        let mut b = vec![0u8; 44];
        b[..4].copy_from_slice(b"MSCF");
        b[16..20].copy_from_slice(&44u32.to_le_bytes());
        b[24..26].copy_from_slice(&[3, 1]);
        b[26..28].copy_from_slice(&1u16.to_le_bytes());
        b[28..30].copy_from_slice(&1u16.to_le_bytes());
        b[40..42].copy_from_slice(&(blocks.len() as u16).to_le_bytes());
        b[42..44].copy_from_slice(&compression.to_le_bytes());
        let size: usize = blocks.iter().map(|v| v.1).sum();
        b.extend_from_slice(&(size as u32).to_le_bytes());
        b.extend_from_slice(&[0; 12]);
        b.extend_from_slice(name.as_bytes());
        b.push(0);
        let start = b.len() as u32;
        b[36..40].copy_from_slice(&start.to_le_bytes());
        for &(payload, size) in blocks {
            let header = [
                (payload.len() as u16).to_le_bytes(),
                (size as u16).to_le_bytes(),
            ]
            .concat();
            b.extend_from_slice(&(checksum(payload) ^ checksum(&header)).to_le_bytes());
            b.extend_from_slice(&header);
            b.extend_from_slice(payload);
        }
        let len = b.len() as u32;
        b[8..12].copy_from_slice(&len.to_le_bytes());
        b
    }

    #[test]
    fn stored_resources_case_and_checksum() {
        let b = fixture("Images\\Test.bmp", &[(b"synthetic", 9)], 0);
        let cab = Cabinet::parse(&b, false).unwrap();
        assert_eq!(cab.resource("images/TEST.BMP").unwrap(), b"synthetic");
        assert_eq!(cab.entries()[0].size(), 9);
        for n in 0..b.len() {
            assert!(Cabinet::parse(&b[..n], false).is_err(), "truncation {n}");
        }
        let mut bad = b;
        *bad.last_mut().unwrap() ^= 1;
        assert!(Cabinet::parse(&bad, false)
            .unwrap_err()
            .contains("checksum"));
        assert_eq!(checksum(&[1, 2, 3, 4, 5, 6, 7]), 0x0403_0201 ^ 0x0005_0607);
    }

    #[test]
    fn mszip_uses_previous_block_dictionary_and_declared_sizes() {
        let source = b"openFSX original synthetic dictionary";
        let mut first = b"CK".to_vec();
        first.extend(miniz_oxide::deflate::compress_to_vec(source, 6));
        // Raw DEFLATE fixture generated from the synthetic source with that
        // source as zlib's preset dictionary. It cannot decode independently.
        let second = b"CK\xcb\x27\x46\x11\x00";
        let b = fixture(
            "test.xml",
            &[(&first, source.len()), (second, source.len())],
            1,
        );
        let cab = Cabinet::parse(&b, false).unwrap();
        assert_eq!(cab.resource("test.xml").unwrap(), source.repeat(2));
        assert!(Cabinet::parse(&fixture("x", &[(second, source.len())], 1), false).is_err());
        for size in [source.len() - 1, source.len() + 1] {
            assert!(Cabinet::parse(&fixture("x", &[(&first, size)], 1), false).is_err());
        }
        let mut trailing = first;
        trailing.push(0);
        assert!(Cabinet::parse(&fixture("x", &[(&trailing, source.len())], 1), false).is_err());
    }

    #[test]
    fn rejects_paths_chains_compression_and_bad_file_ranges() {
        for name in ["../x", "C:\\x", "\\x", "a//b", "a/./b", "a./x", "a /x"] {
            assert!(
                Cabinet::parse(&fixture(name, &[(b"x", 1)], 0), false).is_err(),
                "{name}"
            );
        }
        assert!(resource_key("a\0b").is_err());
        for (offset, value) in [(30, 1u16), (34, 1), (42, 3), (52, 0xffff)] {
            let mut b = fixture("x", &[(b"x", 1)], 0);
            b[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
            assert!(Cabinet::parse(&b, false).is_err());
        }
        let mut b = fixture("x", &[(b"x", 1)], 0);
        b[48..52].copy_from_slice(&1u32.to_le_bytes());
        assert!(Cabinet::parse(&b, false).is_err());
    }

    #[test]
    fn reserved_header_folder_data_and_bounded_trailer() {
        let mut b = fixture("test.xml", &[(b"original synthetic", 18)], 0);
        let old_start = u32_at(&b, 36).unwrap() as usize;
        b.splice(36..36, [3, 0, 2, 3, 11, 22, 33]);
        b.splice(51..51, [44, 55]);
        let start = old_start + 9;
        b[30..32].copy_from_slice(&4u16.to_le_bytes());
        b[16..20].copy_from_slice(&53u32.to_le_bytes());
        b[43..47].copy_from_slice(&(start as u32).to_le_bytes());
        b.splice(start + 8..start + 8, [66, 77, 88]);
        let sum = checksum(&b[start + 4..start + 11]) ^ checksum(&b[start + 11..]);
        b[start..start + 4].copy_from_slice(&sum.to_le_bytes());
        let len = b.len() as u32;
        b[8..12].copy_from_slice(&len.to_le_bytes());
        b.extend_from_slice(b"ignored certificate-like trailer, not trusted");
        assert_eq!(
            Cabinet::parse(&b, false)
                .unwrap()
                .resource("test.xml")
                .unwrap(),
            b"original synthetic"
        );
        // A packed size reaching the trailer must fail, despite physical bytes there.
        b[start + 4..start + 6].copy_from_slice(&40u16.to_le_bytes());
        assert!(Cabinet::parse(&b, false).is_err());
    }
}
