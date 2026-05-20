#![cfg(feature = "arbitrary")]

use proptest::prelude::*;

use crate::arbitrary::arb_table_def;

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 256,
        max_shrink_iters: 5000,
        ..ProptestConfig::default()
    })]

    /// normalize() must be idempotent: applying it twice equals applying it once.
    #[test]
    fn normalize_is_idempotent(table in arb_table_def()) {
        if let Ok(once) = table.normalize() {
            let twice = once
                .clone()
                .normalize()
                .expect("normalize must not fail on already-normalized output");
            prop_assert_eq!(once, twice);
        }
        // If normalize fails (e.g. duplicate index name), the property is vacuously true.
    }
}
