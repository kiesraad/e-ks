use askama::Template;
use axum::response::{IntoResponse, Response};

use crate::{
    AppError, Context, CsbContext, CsbStore, HtmlTemplate,
    csb::examination::{
        extractors::CsbPoliticalGroup,
        pages::CsbAllRestorationsPath,
        structs::{AllCsbCorrections, AllOmissions},
    },
    filters,
};

#[derive(Template)]
#[template(path = "csb/examination/pages/all_restorations.html")]
struct CsbAllRestorationsTemplate {
    political_group: CsbPoliticalGroup,
    omission_count: usize,
    correction_count: usize,
    all_omissions: AllOmissions,
    all_corrections: AllCsbCorrections,
}

pub async fn all_restorations(
    _: CsbAllRestorationsPath,
    context: CsbContext,
    store: CsbStore,
) -> Result<Response, AppError> {
    let political_group = CsbPoliticalGroup::new_from_csb_store(&store);
    let omission_count = store.get_omission_count();
    let correction_count = store.get_correction_count();
    Ok(HtmlTemplate(
        CsbAllRestorationsTemplate {
            all_omissions: store.get_all_omissions(&political_group)?,
            all_corrections: store.get_all_corrections(&political_group, context.session.locale),
            political_group,
            omission_count,
            correction_count,
        },
        context,
    )
    .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::StatusCode;
    use std::collections::BTreeSet;

    use crate::{
        CsbAction, ElectoralDistrict,
        structs::{
            candidate_lists::{CandidateList, CandidateListId},
            common::{PlaceOfResidence, UtcDateTime},
            csb::{Correction, Omission, OmissionCategory, PersonCorrection},
            persons::PersonId,
        },
        test_utils::{response_body_string, sample_person},
    };

    /// Creates an omission with the given category and title.
    async fn create_omission(
        store: &CsbStore,
        category: OmissionCategory,
        title: &str,
    ) -> Result<(), AppError> {
        Omission::new(
            category,
            title.parse().unwrap(),
            "description".parse().unwrap(),
            Some("help_text".parse().unwrap()),
        )
        .create(store)
        .await
    }

    #[tokio::test]
    async fn all_restorations_shows_all_omissions() -> Result<(), AppError> {
        let store = CsbStore::new_for_test();
        let pg_title = "pg title".to_string();

        let candidate_title = "candidate title".to_string();
        let person_id = PersonId::new();
        store.add_person(sample_person(person_id));

        let list_title = "list title".to_string();
        let list_id = CandidateListId::new();
        store.add_candidate_list(CandidateList {
            id: list_id,
            electoral_districts: BTreeSet::from([
                ElectoralDistrict::Utrecht,
                ElectoralDistrict::Groningen,
            ]),
            candidates: vec![person_id],
            created_at: UtcDateTime::now(),
        });

        let dos_title = "declarations of support title".to_string();

        create_omission(&store, OmissionCategory::PoliticalGroup, &pg_title).await?;
        create_omission(
            &store,
            OmissionCategory::CandidateList(vec![list_id]),
            &list_title,
        )
        .await?;
        create_omission(
            &store,
            OmissionCategory::DeclarationsOfSupport(vec![ElectoralDistrict::Utrecht]),
            &dos_title,
        )
        .await?;
        create_omission(
            &store,
            OmissionCategory::Candidate {
                person: person_id,
                lists: vec![list_id],
            },
            &candidate_title,
        )
        .await?;

        let stream_id = store.stream_id;

        let response = all_restorations(
            CsbAllRestorationsPath { stream_id },
            CsbContext::new_test(),
            store,
        )
        .await?;

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;

        // page contains titles
        assert!(body.contains(pg_title.as_str()));
        assert!(body.contains(list_title.as_str()));
        assert!(body.contains(dos_title.as_str()));
        assert!(body.contains(candidate_title.as_str()));

        Ok(())
    }

    #[tokio::test]
    async fn all_restorations_shows_omissions_for_paper_added_candidates_and_lists()
    -> Result<(), AppError> {
        let store = CsbStore::new_for_test();

        // Both the candidate and the list only exist in the corrected
        // projection: they were added during paper corrections.
        let person = sample_person(PersonId::new());
        let person_id = person.id;
        store
            .data
            .write()
            .paper_corrected_data
            .persons
            .insert(person_id, person);
        let list_id = CandidateListId::new();
        store.set_paper_corrected_candidate_list(CandidateList {
            id: list_id,
            electoral_districts: BTreeSet::from([ElectoralDistrict::Groningen]),
            candidates: vec![person_id],
            created_at: UtcDateTime::now(),
        });

        create_omission(
            &store,
            OmissionCategory::Candidate {
                person: person_id,
                lists: vec![list_id],
            },
            "candidate title",
        )
        .await?;
        create_omission(
            &store,
            OmissionCategory::CandidateList(vec![list_id]),
            "list title",
        )
        .await?;

        let response = all_restorations(
            CsbAllRestorationsPath {
                stream_id: store.stream_id,
            },
            CsbContext::new_test(),
            store,
        )
        .await?;

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("candidate title"));
        assert!(body.contains("list title"));

        Ok(())
    }

    #[tokio::test]
    async fn all_restorations_shows_corrections_for_paper_added_candidates() -> Result<(), AppError>
    {
        let store = CsbStore::new_for_test();

        // The candidate only exists in the corrected projection: it was added
        // during paper corrections.
        let person_id = PersonId::new();
        store
            .data
            .write()
            .paper_corrected_data
            .persons
            .insert(person_id, sample_person(person_id));

        store
            .update(CsbAction::UpdateCorrection(Correction::Person(
                person_id,
                PersonCorrection::PlaceOfResidence(PlaceOfResidence::Known(
                    "Amsterdam".to_string(),
                )),
            )))
            .await?;

        let response = all_restorations(
            CsbAllRestorationsPath {
                stream_id: store.stream_id,
            },
            CsbContext::new_test(),
            store,
        )
        .await?;

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        // The corrected place of residence renders next to the struck-through
        // paper-corrected one.
        assert!(body.contains(r#"<s class="imported-value">Juinen</s>"#));
        assert!(body.contains(r#"<strong class="csb-corrected-value">Amsterdam</strong>"#));

        Ok(())
    }
}
