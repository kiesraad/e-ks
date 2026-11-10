/// Event type category grouping specific event keys, used by the audit-log
/// filter dropdowns (app and CSB) to render `<optgroup>`s with fine-grained
/// `<option>`s.
pub struct EventTypeCategory {
    pub key: &'static str,
    pub event_types: &'static [&'static str],
}

/// Assert `table` offers every literal key returned by a `fn key(&self)` in
/// `sources`, so no event becomes unfilterable. Scans the source because the
/// event enums carry data and can't be enumerated.
#[cfg(test)]
pub fn assert_lists_every_key(table: &[EventTypeCategory], sources: &[&str]) {
    let listed: Vec<&str> = table.iter().flat_map(|c| c.event_types).copied().collect();
    let keys: Vec<&str> = sources
        .iter()
        .flat_map(|source| source.split("fn key(&self)").skip(1))
        .filter_map(|rest| rest.split("\n    }").next())
        .flat_map(|body| body.split('"').skip(1).step_by(2))
        .collect();

    assert!(!keys.is_empty(), "no `fn key(&self)` found in the sources");
    for key in keys {
        assert!(
            listed.contains(&key),
            "`{key}` missing from the filter table"
        );
    }
}
