use std::collections::HashSet;

use axum_extra::routing::TypedPath;

use crate::{
    CsbStream, Locale, QueryParamState,
    constants::DEFAULT_DATE_FORMAT,
    csb::examination::{
        extractors::CsbPoliticalGroup,
        structs::{CandidateCorrectionField, PaperCorrected},
    },
    projection::WithCorrections,
    structs::{
        csb::{PersonCorrection, PersonCorrectionDelta},
        persons::{Person, PersonId},
    },
    trans,
};

pub struct PaperCorrectedField {
    pub label: String,
    pub corrected: PaperCorrected,
    pub edit_path: String,
}

pub struct AllCsbCorrections {
    pub general: Vec<PaperCorrectedField>,
    pub candidates: Vec<CandidateCorrections>,
}

pub struct CandidateCorrections {
    /// the most "up-to-date" version of the Person, i.e. including all paper- and csb-corrections
    pub person: Person,
    pub corrections: Vec<PaperCorrectedField>,
}

impl CsbStream {
    pub fn get_all_corrections(
        &self,
        political_group: &CsbPoliticalGroup,
        locale: Locale,
    ) -> AllCsbCorrections {
        let mut candidates: Vec<_> = self
            .get_all_csb_corrected_persons()
            .iter()
            .filter_map(|person| self.compute_corrections(person, political_group, locale))
            .collect();

        candidates.sort_unstable_by(|a, b| {
            a.person
                .name
                .display()
                .cmp(&b.person.name.display())
                .then(a.person.id.cmp(&b.person.id))
        });

        let general = self
            .get_appellation_correction(political_group, locale)
            .into_iter()
            .collect();

        AllCsbCorrections {
            general,
            candidates,
        }
    }

    /// The CSB corrected fields of a single candidate, or `None` when the
    /// candidate no longer exists in the paper-corrected projection (deleted
    /// during paper corrections after being corrected) or has no corrections
    /// left.
    fn compute_corrections(
        &self,
        person: &PersonId,
        political_group: &CsbPoliticalGroup,
        locale: Locale,
    ) -> Option<CandidateCorrections> {
        let fully_corrected = self.get_person(*person, WithCorrections::All)?;
        // Absent for candidates that were added during paper corrections.
        let imported = self.get_person(*person, WithCorrections::None);
        let paper_corrected = self.get_person(*person, WithCorrections::Paper);

        let mut corrections: Vec<_> = self
            .get_person_corrections(person)
            .iter()
            .map(|correction| {
                let (field, corrected_value) = match correction {
                    PersonCorrection::Initials(initials) => {
                        (CandidateCorrectionField::Initials, initials.to_string())
                    }
                    PersonCorrection::LastNamePrefix(prefix) => (
                        CandidateCorrectionField::LastNamePrefix,
                        prefix.as_ref().map(ToString::to_string).unwrap_or_default(),
                    ),
                    PersonCorrection::LastName(last_name) => {
                        (CandidateCorrectionField::LastName, last_name.to_string())
                    }
                    PersonCorrection::DateOfBirth(date_of_birth) => (
                        CandidateCorrectionField::DateOfBirth,
                        date_of_birth.format(DEFAULT_DATE_FORMAT).to_string(),
                    ),
                    PersonCorrection::PlaceOfResidence(place_of_residence) => (
                        CandidateCorrectionField::PlaceOfResidence,
                        place_of_residence.to_string(),
                    ),
                };

                let corrected = PaperCorrected::from_field(
                    imported.as_ref(),
                    paper_corrected.as_ref(),
                    |p: &Person| field.extract(p),
                )
                .with_csb_correction(Some(corrected_value));

                (
                    field,
                    PaperCorrectedField {
                        label: field.label(locale),
                        corrected,
                        edit_path: political_group
                            .correction_person_path_from_all_restorations(person, field)
                            .to_string(),
                    },
                )
            })
            .collect();

        if corrections.is_empty() {
            return None;
        }

        corrections.sort_unstable_by_key(|(field, _)| *field);

        Some(CandidateCorrections {
            person: fully_corrected,
            corrections: corrections.into_iter().map(|(_, field)| field).collect(),
        })
    }

    fn get_person_corrections(&self, person: &PersonId) -> HashSet<PersonCorrection> {
        self.data
            .read()
            .csb_corrected_persons
            .get(person)
            .map(PersonCorrectionDelta::get_corrections)
            .unwrap_or_default()
    }

    fn get_appellation_correction(
        &self,
        political_group: &CsbPoliticalGroup,
        locale: Locale,
    ) -> Option<PaperCorrectedField> {
        // Bound to a local so the read guard is released before
        // `get_appellation` takes the lock again.
        let corrected_appellation = self.data.read().csb_corrected_appellation.clone();

        corrected_appellation.map(|name| PaperCorrectedField {
            label: trans!("political_group.appellation", locale),
            corrected: PaperCorrected::new(
                self.get_appellation(WithCorrections::None),
                self.get_appellation(WithCorrections::Paper),
            )
            .with_csb_correction(Some(name.to_string())),
            edit_path: political_group
                .correction_appellation_path()
                .with_query_params(QueryParamState::redirect_to(
                    political_group.all_restorations_path().to_string(),
                ))
                .to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use crate::{
        AppError,
        CsbAction::{self},
        CsbStore,
        structs::{
            common::{Appellation, Initials, LastName, PlaceOfResidence},
            csb::Correction,
        },
        test_utils::{sample_person, sample_person_with},
    };

    use super::*;
    use crate::CsbUser;

    fn all_corrections(store: &CsbStream) -> AllCsbCorrections {
        store.get_all_corrections(&CsbPoliticalGroup::new_from_csb_store(store), Locale::Nl)
    }

    /// Record a CSB correction on a person.
    async fn correct(
        store: &CsbStream,
        person: PersonId,
        correction: PersonCorrection,
    ) -> Result<(), AppError> {
        store
            .update(
                CsbAction::UpdateCorrection(Correction::Person(person, correction))
                    .by(CsbUser::new_test()),
            )
            .await
    }

    /// The corrections entry of one candidate.
    fn corrections_for(corrections: &AllCsbCorrections, person: PersonId) -> &CandidateCorrections {
        corrections
            .candidates
            .iter()
            .find(|c| c.person.id == person)
            .unwrap_or_else(|| panic!("no corrections for {person}"))
    }

    /// Asserts that the candidate has a correction to `value` with the
    /// expected edit path segment and label.
    fn assert_correction(
        store: &CsbStream,
        candidate: &CandidateCorrections,
        person: PersonId,
        value: &str,
        path_segment: &str,
        label: &str,
    ) {
        let correction = candidate
            .corrections
            .iter()
            .find(|c| c.corrected.csb_corrected.as_deref() == Some(value))
            .unwrap_or_else(|| panic!("no correction to {value:?}"));
        assert_eq!(
            correction.edit_path,
            format!(
                "/csb/examination/{}/correction/person/{}/{}?&redirect_to=%2Fcsb%2Fexamination%2F{}%2Fomissions",
                store.stream_id, person, path_segment, store.stream_id
            )
        );
        assert_eq!(correction.label, label.to_string());
    }

    #[test]
    fn get_all_corrections_no_corrections() {
        let store = CsbStore::new_for_test();

        let corrections = all_corrections(&store);

        assert_eq!(corrections.general.len(), 0);
        assert_eq!(corrections.candidates.len(), 0);
    }

    #[tokio::test]
    async fn get_all_corrections_two_candidates() -> Result<(), AppError> {
        let store = CsbStore::new_for_test();

        let p_id1 = PersonId::new();
        let p_id2 = PersonId::new();

        store.add_person(sample_person(p_id1));
        store.add_person(sample_person(p_id2));

        correct(
            &store,
            p_id1,
            PersonCorrection::Initials(Initials::from_str("A.B.").unwrap()),
        )
        .await?;
        correct(
            &store,
            p_id1,
            PersonCorrection::LastName(LastName::from_str("Smit").unwrap()),
        )
        .await?;
        correct(
            &store,
            p_id2,
            PersonCorrection::PlaceOfResidence(PlaceOfResidence::Known("Amsterdam".to_string())),
        )
        .await?;

        let corrections = all_corrections(&store);

        assert_eq!(corrections.general.len(), 0);
        assert_eq!(corrections.candidates.len(), 2);

        let p1_corrections = corrections_for(&corrections, p_id1);
        let p2_corrections = corrections_for(&corrections, p_id2);

        assert_eq!(p1_corrections.corrections.len(), 2);
        assert_eq!(p2_corrections.corrections.len(), 1);

        assert_correction(
            &store,
            p1_corrections,
            p_id1,
            "A.B.",
            "initials",
            "Voorletters",
        );
        assert_correction(
            &store,
            p1_corrections,
            p_id1,
            "Smit",
            "last-name",
            "Achternaam",
        );
        assert_correction(
            &store,
            p2_corrections,
            p_id2,
            "Amsterdam",
            "place-of-residence",
            "Woonplaats",
        );

        Ok(())
    }

    #[tokio::test]
    async fn get_all_corrections_appellation() -> Result<(), AppError> {
        let store = CsbStore::new_for_test();

        store.data.write().csb_corrected_appellation =
            Some(Appellation::from_str("Gecorrigeerde Partij").unwrap());

        let corrections = all_corrections(&store);

        assert_eq!(corrections.general.len(), 1);
        assert_eq!(corrections.candidates.len(), 0);

        let correction = &corrections.general[0];

        assert_eq!(
            correction.corrected.csb_corrected,
            Some("Gecorrigeerde Partij".to_string())
        );
        assert_eq!(
            correction.edit_path,
            format!(
                "/csb/examination/{}/correction/appellation?&redirect_to=%2Fcsb%2Fexamination%2F{}%2Fomissions",
                store.stream_id, store.stream_id
            )
        );
        assert_eq!(correction.label, "Geregistreerde aanduiding".to_string());

        Ok(())
    }

    /// A candidate created during paper corrections has no imported
    /// counterpart; the correction renders with an empty imported side rather
    /// than failing the whole page.
    #[tokio::test]
    async fn get_all_corrections_for_candidate_added_during_paper_corrections()
    -> Result<(), AppError> {
        let store = CsbStore::new_for_test();

        let person_id = PersonId::new();
        store
            .data
            .write()
            .paper_corrected_data
            .persons
            .insert(person_id, sample_person(person_id));

        correct(
            &store,
            person_id,
            PersonCorrection::Initials(Initials::from_str("A.B.").unwrap()),
        )
        .await?;

        let corrections = all_corrections(&store);

        assert_eq!(corrections.candidates.len(), 1);
        let correction = &corrections.candidates[0].corrections[0];
        assert_eq!(correction.corrected.imported, "");
        assert_eq!(correction.corrected.corrected, "H.A.H.A.");
        assert_eq!(correction.corrected.csb_corrected, Some("A.B.".to_string()));

        Ok(())
    }

    /// A candidate deleted during paper corrections keeps its corrections in
    /// the projection, but there is nothing left to show for it.
    #[tokio::test]
    async fn get_all_corrections_skips_candidate_deleted_during_paper_corrections()
    -> Result<(), AppError> {
        let store = CsbStore::new_for_test();

        let person_id = PersonId::new();
        store.add_person(sample_person(person_id));
        correct(
            &store,
            person_id,
            PersonCorrection::Initials(Initials::from_str("A.B.").unwrap()),
        )
        .await?;
        store
            .data
            .write()
            .paper_corrected_data
            .persons
            .remove(&person_id);

        assert!(all_corrections(&store).candidates.is_empty());

        Ok(())
    }

    #[tokio::test]
    async fn get_all_corrections_rows_are_ordered_by_field() -> Result<(), AppError> {
        let store = CsbStore::new_for_test();

        let person_id = PersonId::new();
        store.add_person(sample_person(person_id));

        // recorded in an order that is not the display order
        correct(
            &store,
            person_id,
            PersonCorrection::PlaceOfResidence(PlaceOfResidence::Known("Amsterdam".to_string())),
        )
        .await?;
        correct(
            &store,
            person_id,
            PersonCorrection::LastName(LastName::from_str("Smit").unwrap()),
        )
        .await?;
        correct(
            &store,
            person_id,
            PersonCorrection::DateOfBirth("15-06-1985".parse().unwrap()),
        )
        .await?;
        correct(
            &store,
            person_id,
            PersonCorrection::Initials(Initials::from_str("A.B.").unwrap()),
        )
        .await?;

        let expected = vec![
            "Voorletters".to_string(),
            "Achternaam".to_string(),
            "Geboortedatum".to_string(),
            "Woonplaats".to_string(),
        ];

        // the projection iterates hash maps, so repeat to catch a random order
        for _ in 0..50 {
            let labels: Vec<_> = all_corrections(&store).candidates[0]
                .corrections
                .iter()
                .map(|c| c.label.clone())
                .collect();
            assert_eq!(labels, expected);
        }

        Ok(())
    }

    #[tokio::test]
    async fn get_all_corrections_candidates_are_ordered_by_name() -> Result<(), AppError> {
        let store = CsbStore::new_for_test();

        let ids = [PersonId::new(), PersonId::new(), PersonId::new()];
        for (id, last_name) in ids.iter().zip(["Vries", "Bakker", "Jansen"]) {
            store.add_person(sample_person_with(*id, None, last_name, None, "H.A."));
            correct(
                &store,
                *id,
                PersonCorrection::Initials(Initials::from_str("A.B.").unwrap()),
            )
            .await?;
        }

        for _ in 0..50 {
            let names: Vec<_> = all_corrections(&store)
                .candidates
                .iter()
                .map(|c| c.person.name.last_name.to_string())
                .collect();
            assert_eq!(names, vec!["Bakker", "Jansen", "Vries"]);
        }

        Ok(())
    }

    /// Each row shows only its own value, as the candidate page does.
    #[tokio::test]
    async fn get_all_corrections_keeps_the_prefix_apart() -> Result<(), AppError> {
        let store = CsbStore::new_for_test();

        let person_id = PersonId::new();
        store.add_person(sample_person_with(
            person_id,
            None,
            "Dijk",
            Some("van"),
            "H.A.",
        ));
        correct(
            &store,
            person_id,
            PersonCorrection::LastName(LastName::from_str("Smit").unwrap()),
        )
        .await?;
        correct(&store, person_id, PersonCorrection::LastNamePrefix(None)).await?;

        let corrections = all_corrections(&store);
        let corrections = &corrections.candidates[0].corrections;

        let prefix = corrections
            .iter()
            .find(|c| c.label == "Voorvoegsel")
            .expect("the prefix correction");
        assert_eq!(prefix.corrected.imported, "van");
        assert_eq!(prefix.corrected.csb_corrected, Some(String::new()));

        let last_name = corrections
            .iter()
            .find(|c| c.label == "Achternaam")
            .expect("the last name correction");
        assert_eq!(last_name.corrected.imported, "Dijk");
        assert_eq!(last_name.corrected.corrected, "Dijk");
        assert_eq!(last_name.corrected.csb_corrected, Some("Smit".to_string()));

        Ok(())
    }
}
