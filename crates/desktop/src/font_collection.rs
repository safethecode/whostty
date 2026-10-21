//! Extract a collection face without subsetting glyphs or changing OpenType tables.
//! Table offsets/checksums follow the OpenType SFNT container specification.
const LIMIT: usize = 64 * 1024 * 1024;

fn checksum(bytes: &[u8]) -> u32 {
    bytes.chunks(4).fold(0u32, |sum, chunk| {
        let mut word = [0; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        sum.wrapping_add(u32::from_be_bytes(word))
    })
}

pub fn extract(bytes: &[u8], index: u32) -> Option<Vec<u8>> {
    ttf_parser::fonts_in_collection(bytes)?;
    let raw = ttf_parser::RawFace::parse(bytes, index).ok()?;
    let offset_at = 12usize.checked_add(usize::try_from(index).ok()?.checked_mul(4)?)?;
    let face_offset =
        u32::from_be_bytes(bytes.get(offset_at..offset_at + 4)?.try_into().ok()?) as usize;
    let version = bytes.get(face_offset..face_offset.checked_add(4)?)?;
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
        let size = size.checked_add((table.length as usize).checked_add(3)? & !3)?;
        (size <= LIMIT).then_some(size)
    })?;
    let mut result = vec![0; length];
    result[..4].copy_from_slice(version);
    result[4..6].copy_from_slice(&(count as u16).to_be_bytes());
    let selector = count.ilog2() as u16;
    let range = (1u16 << selector) * 16;
    result[6..8].copy_from_slice(&range.to_be_bytes());
    result[8..10].copy_from_slice(&selector.to_be_bytes());
    result[10..12].copy_from_slice(&((count as u16 * 16) - range).to_be_bytes());
    let mut offset = header;
    let mut head = None;
    for (index, table) in tables.iter().enumerate() {
        let data = raw.table(table.tag)?;
        let end = offset.checked_add(data.len())?;
        result.get_mut(offset..end)?.copy_from_slice(data);
        if table.tag.to_bytes() == *b"head" {
            if data.len() < 12 {
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
