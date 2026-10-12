use super::{axml, fixture::*, guarded_child};

#[test]
fn string_count_is_checked_before_reserve() {
    guarded_child("pool-count");
}

#[test]
fn truncated_document_is_not_a_partial_success() {
    let bytes = manifest("com.example.valid");
    for cut in [8, 20, 40, bytes.len() - 1] {
        assert!(axml::parse(&bytes[..cut]).is_none(), "cut={cut}");
    }
}

#[test]
fn root_header_and_total_are_checked() {
    let original = manifest("com.example.valid");
    for (offset, value) in [(2, 4), (2, u16::MAX as u32), (4, 0), (4, u32::MAX)] {
        let mut bytes = original.clone();
        if offset == 2 {
            bytes[offset..offset + 2].copy_from_slice(&(value as u16).to_le_bytes());
        } else {
            put_u32(&mut bytes, offset, value);
        }
        assert!(
            axml::parse(&bytes).is_none(),
            "offset={offset} value={value}"
        );
    }
}

macro_rules! bad_pool_field {
    ($name:ident, $at:expr, $value:expr) => {
        #[test]
        fn $name() {
            let mut chunk = pool(&["manifest", "package", "com.example.valid"], true);
            put_u32(&mut chunk, $at, $value);
            assert!(axml::parse(&document(&[chunk])).is_none());
        }
    };
}
bad_pool_field!(string_offsets_must_fit_before_string_data, 8, 128);
bad_pool_field!(style_offsets_must_fit_before_string_data, 12, u32::MAX);
bad_pool_field!(strings_cannot_overlap_header_or_tables, 20, 28);
bad_pool_field!(strings_start_cannot_escape_chunk, 20, u32::MAX);
bad_pool_field!(styles_cannot_overlap_header_or_tables, 24, 16);
bad_pool_field!(string_offset_cannot_escape_chunk, 28, u32::MAX);

#[test]
fn chunk_headers_and_sizes_must_fit() {
    for (at, value) in [(2, 4), (2, u16::MAX as u32), (4, 4), (4, u32::MAX)] {
        let mut chunk = pool(&["abc"], true);
        if at == 2 {
            chunk[at..at + 2].copy_from_slice(&(value as u16).to_le_bytes());
        } else {
            put_u32(&mut chunk, at, value);
        }
        assert!(axml::parse(&document(&[chunk])).is_none(), "{at}:{value}");
    }
}

#[test]
fn attribute_table_must_fit_in_its_chunk() {
    for (at, value) in [(24, 0u16), (24, u16::MAX), (26, 0), (26, 4), (28, 2)] {
        let mut chunk = element(0, &[(1, 2, 3, 2)]);
        chunk[at..at + 2].copy_from_slice(&value.to_le_bytes());
        let bytes = document(&[
            pool(&["manifest", "package", "com.example.valid"], true),
            chunk,
        ]);
        assert!(axml::parse(&bytes).is_none(), "{at}:{value}");
    }
}

#[test]
fn resource_map_rejects_partial_words() {
    let malformed_map = vec![0x80, 1, 8, 0, 9, 0, 0, 0, 0];
    assert!(axml::parse(&document(&[pool(&["abc"], true), malformed_map])).is_none());
}

#[test]
fn malformed_tail_does_not_salvage_prior_metadata() {
    let mut bytes = manifest("com.example.valid");
    bytes.extend_from_slice(&[1, 2, 3]);
    let size = bytes.len() as u32;
    put_u32(&mut bytes, 4, size);
    assert!(axml::parse(&bytes).is_none());
}

#[test]
fn utf16_and_utf8_non_ascii_labels_survive() {
    for utf8 in [false, true] {
        let bytes = document(&[
            pool(&["application", "label", "VR-пример 🥽"], utf8),
            element(0, &[(1, 2, 3, 2)]),
            end_element(0),
        ]);
        let doc = axml::parse(&bytes).unwrap();
        assert_eq!(
            doc.find("application").unwrap().attr("label"),
            Some("VR-пример 🥽")
        );
    }
}
