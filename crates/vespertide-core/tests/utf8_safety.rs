use proptest::prelude::*;
use vespertide_core::MigrationAction;

proptest! {
    #[test]
    fn action_display_does_not_panic_on_unicode(
        s in proptest::collection::vec(any::<char>(), 0..100)
            .prop_map(|v| v.into_iter().collect::<String>())
    ) {
        let action = MigrationAction::RawSql { sql: s };

        let _ = format!("{action:?}");
        let _ = format!("{action}");
    }
}
