use super::PaperCorrected;
use crate::{CsbStream, projection::WithCorrections};

/// A name authorisation with its rows diffed against the corrections.
pub struct PaperCorrectedNameAuthorisation {
    pub heading: String,
    pub legal_name: PaperCorrected,
    pub authorised_agent: PaperCorrected,
}

/// Pair the imported name authorisations with their corrected counterparts
/// by id; entities added by the corrections are appended, entities deleted by
/// the corrections are hidden.
pub fn paper_corrected_name_authorisations(
    store: &CsbStream,
) -> Vec<PaperCorrectedNameAuthorisation> {
    let imported = store.get_name_authorisations(WithCorrections::None);
    let corrected = store.get_name_authorisations(WithCorrections::Paper);

    let mut rows: Vec<PaperCorrectedNameAuthorisation> = imported
        .iter()
        .filter_map(|na| {
            let counterpart = corrected.iter().find(|c| c.id == na.id)?;
            Some(PaperCorrectedNameAuthorisation {
                heading: counterpart.legal_name.to_string(),
                legal_name: PaperCorrected::from_field(Some(na), Some(counterpart), |n| {
                    n.legal_name.to_string()
                }),
                authorised_agent: PaperCorrected::from_field(Some(na), Some(counterpart), |n| {
                    n.name.display()
                }),
            })
        })
        .collect();

    rows.extend(
        corrected
            .iter()
            .filter(|c| !imported.iter().any(|na| na.id == c.id))
            .map(|c| PaperCorrectedNameAuthorisation {
                heading: c.legal_name.to_string(),
                legal_name: PaperCorrected::from_field(None, Some(c), |n| n.legal_name.to_string()),
                authorised_agent: PaperCorrected::from_field(None, Some(c), |n| n.name.display()),
            }),
    );

    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CsbStore, structs::name_authorisations::NameAuthorisationId,
        test_utils::sample_name_authorisation,
    };

    #[test]
    fn name_authorisation_deleted_by_the_corrections_is_hidden() {
        let store = CsbStore::new_for_test();
        let kept = sample_name_authorisation(NameAuthorisationId::new());
        let deleted = sample_name_authorisation(NameAuthorisationId::new());
        store.edit(|data| {
            data.imported_mut()
                .name_authorisations
                .insert(kept.id, kept.clone());
            data.imported_mut()
                .name_authorisations
                .insert(deleted.id, deleted);
            data.paper_corrected_mut()
                .name_authorisations
                .insert(kept.id, kept.clone());
        });

        let rows = paper_corrected_name_authorisations(&store);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].heading, kept.legal_name.to_string());
    }
}
