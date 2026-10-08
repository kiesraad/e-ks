/// Event type category grouping specific event keys, used by the audit-log
/// filter dropdowns (app and CSB) to render `<optgroup>`s with fine-grained
/// `<option>`s.
pub struct EventTypeCategory {
    pub key: &'static str,
    pub event_types: &'static [&'static str],
}

/// Check that `table` lists every category and key `events` can produce, so
/// the filter dropdown offers each event the log can hold. A group may list
/// its own category instead of keys, to filter on the category as a whole.
/// Call it with one event per variant.
#[cfg(test)]
pub(crate) fn assert_covers<E: crate::Event>(
    table: &[EventTypeCategory],
    events: impl IntoIterator<Item = E>,
) {
    for event in events {
        let (category, key) = (event.category(), event.key());
        let group = table
            .iter()
            .find(|group| group.key == category)
            .unwrap_or_else(|| panic!("category `{category}` is missing from the filter table"));
        assert!(
            group.event_types.contains(&key) || group.event_types.contains(&category),
            "event `{key}` is missing under category `{category}` in the filter table"
        );
    }
}
