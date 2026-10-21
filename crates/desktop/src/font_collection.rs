//! Extract a collection face without subsetting glyphs or changing OpenType tables.
//! Table offsets/checksums follow the OpenType SFNT container specification.
const LIMIT: usize = 64 * 1024 * 1024;
use std::{
    io::{Read, Seek, SeekFrom},
    path::Path,
};

fn checksum(bytes: &[u8]) -> u32 {
    bytes.chunks(4).fold(0u32, |sum, chunk| {
        let mut word = [0; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        sum.wrapping_add(u32::from_be_bytes(word))
    })
}

pub fn extract(bytes: &[u8], index: u32) -> Option<Vec<u8>> {
    extract_with(bytes.len(), index, |offset, output| {
        output.copy_from_slice(bytes.get(offset..offset.checked_add(output.len())?)?);
        Some(())
    })
}

/// Read only the chosen face's tables, avoiding a large temporary collection buffer.
pub fn read(path: &Path, index: u32) -> std::io::Result<Option<Vec<u8>>> {
    let mut file = std::fs::File::open(path)?;
    let length = file.metadata()?.len();
    if length > LIMIT as u64 {
        return Err(std::io::Error::other("Font files must be at most 64 MiB"));
    }
    Ok(extract_with(length as usize, index, |offset, output| {
        file.seek(SeekFrom::Start(offset as u64)).ok()?;
        file.read_exact(output).ok()
    }))
}

fn extract_with(
    length: usize,
    index: u32,
    mut read: impl FnMut(usize, &mut [u8]) -> Option<()>,
) -> Option<Vec<u8>> {
    let mut read = |offset: usize, output: &mut [u8]| {
        (offset.checked_add(output.len())? <= length).then_some(())?;
        read(offset, output)
    };
    let mut collection = [0; 12];
    read(0, &mut collection)?;
    if &collection[..4] != b"ttcf"
        || index >= u32::from_be_bytes(collection[8..12].try_into().ok()?)
    {
        return None;
    }
    let offset_at = 12usize.checked_add(usize::try_from(index).ok()?.checked_mul(4)?)?;
    let mut offset_bytes = [0; 4];
    read(offset_at, &mut offset_bytes)?;
    let face_offset = u32::from_be_bytes(offset_bytes) as usize;
    let mut face_header = [0; 12];
    read(face_offset, &mut face_header)?;
    let count = u16::from_be_bytes(face_header[4..6].try_into().ok()?) as usize;
    if count == 0 || count > 4095 {
        return None;
    }
    let mut directory = vec![0; 12 + count * 16];
    read(face_offset, &mut directory)?;
    let raw = ttf_parser::RawFace::parse(&directory, 0).ok()?;
    // Container rewriting invalidates a digital signature; preserve every other table.
    let tables: Vec<_> = raw
        .table_records
        .into_iter()
        .filter(|r| r.tag.to_bytes() != *b"DSIG")
        .collect();
    let count = tables.len();
    if count == 0 || count > 4095 || tables.windows(2).any(|pair| pair[0].tag >= pair[1].tag) {
        return None;
    }
    let header = 12 + count * 16;
    let length = tables.iter().try_fold(header, |size, table| {
        ((table.offset as usize).checked_add(table.length as usize)? <= length).then_some(())?;
        let size = size.checked_add((table.length as usize).checked_add(3)? & !3)?;
        (size <= LIMIT).then_some(size)
    })?;
    let mut result = vec![0; length];
    result[..4].copy_from_slice(&face_header[..4]);
    result[4..6].copy_from_slice(&(count as u16).to_be_bytes());
    let selector = count.ilog2() as u16;
    let range = (1u16 << selector) * 16;
    result[6..8].copy_from_slice(&range.to_be_bytes());
    result[8..10].copy_from_slice(&selector.to_be_bytes());
    result[10..12].copy_from_slice(&((count as u16 * 16) - range).to_be_bytes());
    let mut offset = header;
    let mut head = None;
    for (index, table) in tables.iter().enumerate() {
        let end = offset.checked_add(table.length as usize)?;
        read(table.offset as usize, result.get_mut(offset..end)?)?;
        if table.tag.to_bytes() == *b"head" {
            if table.length < 12 {
                return None;
            }
            result[offset + 8..offset + 12].fill(0);
            head = Some(offset);
        }
        let entry = 12 + index * 16;
        let sum = checksum(&result[offset..end]);
        result[entry..entry + 4].copy_from_slice(&table.tag.to_bytes());
        result[entry + 4..entry + 8].copy_from_slice(&sum.to_be_bytes());
        result[entry + 8..entry + 12].copy_from_slice(&(offset as u32).to_be_bytes());
        result[entry + 12..entry + 16].copy_from_slice(&table.length.to_be_bytes());
        offset = (end + 3) & !3;
    }
    let head = head?;
    let adjustment = 0xB1B0_AFBAu32.wrapping_sub(checksum(&result));
    result[head + 8..head + 12].copy_from_slice(&adjustment.to_be_bytes());
    ttf_parser::Face::parse(&result, 0).ok()?;
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extraction_preserves_glyphs_shaping_and_checksums_and_rejects_bad_offsets() {
        let font = include_bytes!("../tests/fixtures/shaping.ttf");
        let mut collection = Vec::from(&b"ttcf\0\x01\0\0\0\0\0\x01\0\0\0\x10"[..]);
        collection.extend_from_slice(font);
        let raw = ttf_parser::RawFace::parse(font, 0).unwrap();
        for (index, table) in raw.table_records.into_iter().enumerate() {
            let at = 16 + 12 + index * 16 + 8;
            collection[at..at + 4].copy_from_slice(&(table.offset + 16).to_be_bytes());
        }
        collection.resize(collection.len() + 1024 * 1024, 0);
        let extracted = extract(&collection, 0).unwrap();
        let mut read_bytes = 0;
        assert_eq!(
            extract_with(collection.len(), 0, |offset, output| {
                read_bytes += output.len();
                output.copy_from_slice(collection.get(offset..offset + output.len())?);
                Some(())
            })
            .unwrap(),
            extracted
        );
        assert!(read_bytes < 16384, "Read unused collection data");
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), &collection).unwrap();
        assert_eq!(read(file.path(), 0).unwrap().unwrap(), extracted);
        assert!(extracted.len() < 16384);
        assert_eq!(checksum(&extracted), 0xB1B0_AFBA);
        let output = ttf_parser::RawFace::parse(&extracted, 0).unwrap();
        for table in raw.table_records {
            if table.tag.to_bytes() != *b"head" && table.tag.to_bytes() != *b"DSIG" {
                assert_eq!(raw.table(table.tag), output.table(table.tag));
            }
        }
        assert!(ab_glyph::FontRef::try_from_slice(&extracted).is_ok());
        assert!(rustybuzz::Face::from_slice(&extracted, 0).is_some());
        assert!(extract(&collection, 1).is_none());
        collection[36..40].fill(0xff);
        assert!(extract(&collection, 0).is_none());
    }
}
