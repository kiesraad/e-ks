use crate::{
    AnyLocale, CsbStream,
    csb::examination::structs::{BrpCheckState, RestorationStatus},
    projection::WithCorrections,
    structs::{candidate_lists::CandidateList, persons::Person},
};

use super::paper_corrected::PaperCorrected;

/// A candidate row on the CSB candidate list examination page.
pub struct CsbCandidate {
    pub person: Person,
    pub position: PaperCorrected,
    pub name: PaperCorrected,
    pub residence: PaperCorrected,
    pub brp: BrpCheckState,
    pub restoration_status: RestorationStatus,
    /// Whether an unresolved omission scraps this candidate from the list.
    /// Only rendered in the recovery ("Herstelde lijsten") phase.
    pub is_scrapped: bool,
    /// The candidate's number in the recovery ("Herstelde lijsten") phase,
    /// which runs over the candidates that are not scrapped. `None` for a
    /// scrapped candidate: it is still listed, without a number.
    pub recovery_position: Option<usize>,
}

impl CsbCandidate {
    /// Build the rows for a candidate list, ordered by paper-corrected
    /// position. Candidates removed by the corrections keep their imported
    /// position in the ordering.
    pub fn rows_for_list(
        store: &CsbStream,
        list: &CandidateList,
        locale: AnyLocale,
    ) -> Vec<CsbCandidate> {
        let mut rows = imported_rows(store, list, locale);
        rows.extend(corrected_only_rows(store, list, locale));
        rows.sort_by_key(|(position, _)| *position);
        rows.into_iter().map(|(_, row)| row).collect()
    }
}

/// Rows for the imported candidates, keyed by their corrected position (or
/// their imported position when the corrections removed them).
fn imported_rows(
    store: &CsbStream,
    list: &CandidateList,
    locale: AnyLocale,
) -> Vec<(usize, CsbCandidate)> {
    list.candidates
        .iter()
        .enumerate()
        .filter_map(|(index, person_id)| {
            let person = store.get_person(*person_id, WithCorrections::None)?;
            let corrected = store.get_person(*person_id, WithCorrections::Paper);
            let csb_corrected = store.get_person(*person_id, WithCorrections::All);
            let corrected_position =
                store.get_candidate_position(list.id, *person_id, WithCorrections::All);

            Some((
                corrected_position.unwrap_or(index + 1),
                CsbCandidate {
                    position: PaperCorrected::new(
                        (index + 1).to_string(),
                        corrected_position
                            .map(|p| p.to_string())
                            .unwrap_or_default(),
                    ),
                    name: PaperCorrected::new(
                        name_string(&person, locale),
                        corrected
                            .as_ref()
                            .map(|p| name_string(p, locale))
                            .unwrap_or_default(),
                    )
                    .with_csb_correction(csb_corrected.as_ref().map(|p| name_string(p, locale))),
                    residence: PaperCorrected::new(
                        residence_string(&person),
                        corrected.as_ref().map(residence_string).unwrap_or_default(),
                    )
                    .with_csb_correction(csb_corrected.as_ref().map(residence_string)),
                    restoration_status: RestorationStatus::for_candidate(store, person.id, list.id),
                    brp: BrpCheckState::for_candidate(store, person.id),
                    is_scrapped: store.is_candidate_scrapped(person.id, list.id),
                    recovery_position: store.get_recovery_position(list.id, person.id),
                    person,
                },
            ))
        })
        .collect()
}

/// Rows for candidates the paper corrections added to the list, keyed by
/// their corrected position.
fn corrected_only_rows(
    store: &CsbStream,
    list: &CandidateList,
    locale: AnyLocale,
) -> Vec<(usize, CsbCandidate)> {
    let Some(corrected_list) = store.get_candidate_list(list.id, WithCorrections::All) else {
        return Vec::new();
    };

    corrected_list
        .candidates
        .iter()
        .enumerate()
        .filter(|(_, id)| !list.candidates.contains(id))
        .filter_map(|(index, person_id)| {
            let person = store.get_person(*person_id, WithCorrections::All)?;
            let csb_corrected = store.get_person(*person_id, WithCorrections::All);
            Some((
                index + 1,
                CsbCandidate {
                    position: PaperCorrected::new(String::new(), (index + 1).to_string()),
                    name: PaperCorrected::new(String::new(), name_string(&person, locale))
                        .with_csb_correction(
                            csb_corrected.as_ref().map(|p| name_string(p, locale)),
                        ),
                    residence: PaperCorrected::new(String::new(), residence_string(&person))
                        .with_csb_correction(csb_corrected.as_ref().map(residence_string)),
                    brp: BrpCheckState::for_candidate(store, person.id),
                    restoration_status: RestorationStatus::for_candidate(store, person.id, list.id),
                    is_scrapped: store.is_candidate_scrapped(person.id, list.id),
                    recovery_position: store.get_recovery_position(list.id, person.id),
                    person,
                },
            ))
        })
        .collect()
}

fn name_string(person: &Person, locale: AnyLocale) -> String {
    format!(
        "{}, {}",
        person.name.last_name_with_prefix_appended(),
        person.initials_as_printed_on_list(locale)
    )
}

fn residence_string(person: &Person) -> String {
    let place = person
        .personal_data
        .place_of_residence
        .as_ref()
        .map(ToString::to_string)
        .unwrap_or_default();

    match &person.personal_data.country {
        Some(country) if !person.lives_in_nl() => format!("{place} ({country})"),
        _ => place,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CsbStore,
        structs::{candidate_lists::CandidateListId, persons::PersonId},
        test_utils::{sample_candidate_list, sample_person_with_last_name},
    };

    fn store_with_imported_list(candidates: &[PersonId]) -> (CsbStore, CandidateList) {
        let store = CsbStore::new_for_test();
        let list_id = CandidateListId::new();
        let mut list = sample_candidate_list(list_id);
        list.candidates = candidates.to_vec();
        for (index, id) in candidates.iter().enumerate() {
            store.add_person(sample_person_with_last_name(*id, &format!("P{index}")));
        }
        store.add_candidate_list(list.clone());
        (store, list)
    }

    #[test]
    fn rows_follow_the_corrected_order() {
        let (a, b) = (PersonId::new(), PersonId::new());
        let (store, list) = store_with_imported_list(&[a, b]);

        let mut corrected_list = list.clone();
        corrected_list.candidates = vec![b, a];
        store.set_paper_corrected_candidate_list(corrected_list);

        let rows = CsbCandidate::rows_for_list(&store, &list, AnyLocale::En);

        assert_eq!(
            rows.iter().map(|r| r.person.id).collect::<Vec<_>>(),
            vec![b, a]
        );
        assert_eq!(rows[0].position.imported, "2");
        assert_eq!(rows[0].position.corrected, "1");
    }

    #[test]
    fn removed_candidates_keep_their_imported_position_in_the_order() {
        let (a, b, c, d) = (
            PersonId::new(),
            PersonId::new(),
            PersonId::new(),
            PersonId::new(),
        );
        let (store, list) = store_with_imported_list(&[a, b, c]);

        // The corrections remove B and insert D in its place.
        let mut corrected_list = list.clone();
        corrected_list.candidates = vec![a, d, c];
        store.set_paper_corrected_candidate_list(corrected_list);
        store.add_person(sample_person_with_last_name(d, "Nieuw"));

        let rows = CsbCandidate::rows_for_list(&store, &list, AnyLocale::En);

        assert_eq!(
            rows.iter().map(|r| r.person.id).collect::<Vec<_>>(),
            vec![a, b, d, c]
        );
        // B was removed: imported position only.
        assert_eq!(rows[1].position.imported, "2");
        assert_eq!(rows[1].position.corrected, "");
        // D was added: corrected position only.
        assert_eq!(rows[2].position.imported, "");
        assert_eq!(rows[2].position.corrected, "2");
    }
}
