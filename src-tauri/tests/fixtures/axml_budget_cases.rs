use super::{axml, fixture::*, guarded_child};

macro_rules! budget_test {
    ($name:ident, $case:literal) => {
        #[test]
        fn $name() {
            guarded_child($case);
        }
    };
}
budget_test!(
    repeated_offsets_cannot_amplify_decoded_pool,
    "axml-pool-bytes"
);
budget_test!(
    repeated_attributes_cannot_amplify_cloned_values,
    "axml-clone-bytes"
);
budget_test!(
    single_utf16_string_has_a_decoded_limit,
    "axml-single-string"
);
budget_test!(
    string_count_has_a_policy_limit_even_when_table_fits,
    "axml-string-count"
);
budget_test!(element_count_has_a_policy_limit, "axml-element-count");
budget_test!(total_attributes_have_a_policy_limit, "axml-attribute-count");
budget_test!(total_chunks_have_a_policy_limit, "axml-chunk-count");

#[test]
fn utf8_and_utf16_strings_need_a_terminator() {
    for utf8 in [false, true] {
        let mut chunk = pool(&["abc"], utf8);
        let null = if utf8 { 32 + 2 + 3 } else { 32 + 2 + 6 };
        chunk[null] = 1;
        assert!(axml::parse(&document(&[chunk])).is_none(), "utf8={utf8}");
    }
}

#[test]
fn utf8_length_prefix_must_match_utf16_units() {
    let mut chunk = pool(&["abc"], true);
    chunk[32] = 9;
    assert!(axml::parse(&document(&[chunk])).is_none());
}

#[test]
fn typed_string_indices_cannot_silently_become_empty() {
    let bytes = document(&[
        pool(&["manifest", "package"], true),
        element(0, &[(1, u32::MAX, 3, 99)]),
    ]);
    assert!(axml::parse(&bytes).is_none());
}
