use crate::{pg::request_extractor, structs::political_groups::PoliticalGroup};

request_extractor!(PoliticalGroup, |store, parts, state| {
    Ok(store.snapshot().get_political_group().clone())
});
