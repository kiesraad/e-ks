use axum::{Router, response::Response};
use axum_extra::routing::RouterExt;

use crate::{AppError, AppRequestState, CsbContext, CsbStore, structs::csb::CsbPhase};

use super::paths::{
    CsbRecoveryCandidateListPath, CsbRecoveryCandidatePath, CsbRecoveryGeneralInformationPath,
    CsbRecoveryPoliticalGroupPath,
};

mod omissions;
mod overview;
mod set_status;

pub fn router<S: AppRequestState>() -> Router<S> {
    Router::new()
        .typed_get(overview::overview)
        .typed_get(political_group)
        .typed_get(general_information)
        .typed_get(candidate_list)
        .typed_get(candidate)
        .typed_get(omissions::omissions)
        .typed_post(set_status::set_status)
}

// The mirrored detail pages: the examination pages rendered in recovery mode,
// which hides the examination-only actions and shows the assessment controls.

async fn political_group(
    _: CsbRecoveryPoliticalGroupPath,
    context: CsbContext,
    store: CsbStore,
) -> Result<Response, AppError> {
    crate::csb::examination::pages::political_group::render(context, store, CsbPhase::Recovery)
        .await
}

async fn general_information(
    _: CsbRecoveryGeneralInformationPath,
    context: CsbContext,
    store: CsbStore,
) -> Result<Response, AppError> {
    crate::csb::examination::pages::general_information::render(context, store, CsbPhase::Recovery)
        .await
}

async fn candidate_list(
    CsbRecoveryCandidateListPath { list_id, .. }: CsbRecoveryCandidateListPath,
    context: CsbContext,
    store: CsbStore,
) -> Result<Response, AppError> {
    crate::csb::examination::pages::candidate_list::render(
        list_id,
        context,
        store,
        CsbPhase::Recovery,
    )
    .await
}

async fn candidate(
    CsbRecoveryCandidatePath {
        list_id, person_id, ..
    }: CsbRecoveryCandidatePath,
    context: CsbContext,
    store: CsbStore,
) -> Result<Response, AppError> {
    crate::csb::examination::pages::candidate::render(
        list_id,
        person_id,
        context,
        store,
        CsbPhase::Recovery,
    )
    .await
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;
    use axum::{http::StatusCode, response::IntoResponse};

    use crate::{
        CsbAction, PgEvent,
        structs::{
            candidate_lists::CandidateListId,
            csb::{Omission, OmissionCategory, OmissionStatus, OmissionText, OmissionTitle},
            name_authorisations::NameAuthorisationId,
            persons::PersonId,
        },
        test_utils::{
            response_body_string, sample_candidate_list, sample_name_authorisation, sample_person,
            sample_political_group,
        },
    };

    #[tokio::test]
    async fn recovery_political_group_page_hides_examination_actions() {
        let store = CsbStore::new_for_test();
        store.set_political_group(sample_political_group());
        let stream_id = store.stream_id;

        let response = political_group(
            CsbRecoveryPoliticalGroupPath { stream_id },
            CsbContext::new_test(),
            store,
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        // The page links within the recovery phase, not the examination phase.
        assert!(body.contains(&format!("/csb/recovery/{stream_id}")));
        assert!(!body.contains("/csb/examination/"));
        // The examination-only actions are gone.
        assert!(!body.contains("paper-corrections"));
        assert!(!body.contains("toggle-finish"));
        assert!(!body.contains("/omission/declarations-of-support/"));
    }

    #[tokio::test]
    async fn recovery_candidate_list_shows_controls_and_scraps_candidates() {
        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;

        let person = sample_person(PersonId::new());
        let person_id = person.id;
        let list_id = CandidateListId::new();
        let mut list = sample_candidate_list(list_id);
        list.candidates = vec![person_id];
        store.add_person(person);
        store.add_candidate_list(list);

        let omission = Omission::new(
            OmissionCategory::Candidate {
                person: person_id,
                lists: vec![list_id],
            },
            "Missing consent".parse().unwrap(),
            "The declaration of consent is missing.".parse().unwrap(),
            None,
        );
        omission.create(&store).await.unwrap();
        omission
            .set_status(&store, OmissionStatus::NotRecovered)
            .await
            .unwrap();

        let response = candidate_list(
            CsbRecoveryCandidateListPath { stream_id, list_id },
            CsbContext::new_test(),
            store,
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        // No omissions can be added, but the list's own omissions would get
        // status controls (this one belongs to the candidate, so none here).
        assert!(!body.contains("/csb/examination/"));
        // The unrecovered omission scraps the candidate: struck through + tag.
        assert!(body.contains("<s class=\"imported-value\">"));
        assert!(body.contains("Scrapped"));
    }

    #[tokio::test]
    async fn recovery_candidate_list_renumbers_around_a_scrapped_candidate() {
        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;

        let scrapped = sample_person(PersonId::new());
        let kept = sample_person(PersonId::new());
        let scrapped_id = scrapped.id;
        let list_id = CandidateListId::new();
        let mut list = sample_candidate_list(list_id);
        list.candidates = vec![scrapped_id, kept.id];
        store.add_person(scrapped);
        store.add_person(kept);
        store.add_candidate_list(list);

        let omission = Omission::new(
            OmissionCategory::Candidate {
                person: scrapped_id,
                lists: vec![list_id],
            },
            "Missing consent".parse().unwrap(),
            "The declaration of consent is missing.".parse().unwrap(),
            None,
        );
        omission.create(&store).await.unwrap();
        omission
            .set_status(&store, OmissionStatus::NotRecovered)
            .await
            .unwrap();

        let response = candidate_list(
            CsbRecoveryCandidateListPath { stream_id, list_id },
            CsbContext::new_test(),
            store,
        )
        .await
        .unwrap()
        .into_response();

        let body = response_body_string(response).await;
        // Both candidates are listed, but only the one that survives is
        // numbered, and it takes the number of the scrapped candidate above it.
        assert_eq!(body.matches("position-badge").count(), 1);
        assert!(body.contains(">1</span>"));
    }

    #[tokio::test]
    async fn recovery_candidate_list_is_scrapped_in_all_its_districts() {
        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;

        let person = sample_person(PersonId::new());
        let list_id = CandidateListId::new();
        let mut list = sample_candidate_list(list_id);
        list.candidates = vec![person.id];
        store.add_person(person);
        store.add_candidate_list(list.clone());

        let omission = Omission::new(
            OmissionCategory::DeclarationsOfSupport(list.electoral_districts.clone()),
            "Too few declarations".parse().unwrap(),
            "Not enough declarations of support were handed in."
                .parse()
                .unwrap(),
            None,
        );
        omission.create(&store).await.unwrap();
        omission
            .set_status(&store, OmissionStatus::NotRecovered)
            .await
            .unwrap();

        let response = candidate_list(
            CsbRecoveryCandidateListPath { stream_id, list_id },
            CsbContext::new_test(),
            store,
        )
        .await
        .unwrap()
        .into_response();

        let body = response_body_string(response).await;

        assert!(body.contains("<s class=\"imported-value\">"));
    }

    #[tokio::test]
    async fn recovery_candidate_page_is_read_only_with_status_panel() {
        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;

        let person = sample_person(PersonId::new());
        let person_id = person.id;
        let list_id = CandidateListId::new();
        let mut list = sample_candidate_list(list_id);
        list.candidates = vec![person_id];
        store.add_person(person);
        store.add_candidate_list(list);

        Omission::new(
            OmissionCategory::Candidate {
                person: person_id,
                lists: vec![list_id],
            },
            "Missing consent".parse().unwrap(),
            "The declaration of consent is missing.".parse().unwrap(),
            None,
        )
        .create(&store)
        .await
        .unwrap();

        let response = candidate(
            CsbRecoveryCandidatePath {
                stream_id,
                list_id,
                person_id,
            },
            CsbContext::new_test(),
            store,
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        // The omission gets its status control...
        assert!(body.contains(&format!("/csb/recovery/{stream_id}/omission/")));
        assert!(body.contains(r#"value="recovered""#));
        // ...while the correction links and add-omission dialog are gone.
        assert!(!body.contains("/correction/"));
        assert!(!body.contains("/csb/examination/"));
    }

    #[tokio::test]
    async fn recovery_candidate_list_page_decides_a_multi_list_omission_per_list() {
        use crate::ElectoralDistrict;

        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;

        let mut lists = Vec::new();
        for district in [ElectoralDistrict::Groningen, ElectoralDistrict::Utrecht] {
            let list_id = CandidateListId::new();
            let mut list = sample_candidate_list(list_id);
            list.electoral_districts = vec![district];
            store.add_candidate_list(list);
            lists.push(list_id);
        }

        Omission::new(
            OmissionCategory::CandidateList(lists.clone()),
            "Too many candidates".parse().unwrap(),
            "The list holds more candidates than allowed."
                .parse()
                .unwrap(),
            None,
        )
        .create(&store)
        .await
        .unwrap();

        let response = candidate_list(
            CsbRecoveryCandidateListPath {
                stream_id,
                list_id: lists[0],
            },
            CsbContext::new_test(),
            store,
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        // The page decides its own list only; the other list has its own page.
        assert_eq!(body.matches(r#"name="candidate_list""#).count(), 1);
        assert!(body.contains(&format!(r#"value="{}""#, lists[0])));
        assert!(!body.contains(&format!(r#"value="{}""#, lists[1])));
        assert!(!body.contains("omission-part-table"));
    }

    #[tokio::test]
    async fn recovery_candidate_page_decides_a_multi_list_omission_per_list() {
        use crate::ElectoralDistrict;

        let store = CsbStore::new_for_test();
        let stream_id = store.stream_id;

        let person = sample_person(PersonId::new());
        let person_id = person.id;
        store.add_person(person);

        let mut lists = Vec::new();
        for district in [ElectoralDistrict::Groningen, ElectoralDistrict::Utrecht] {
            let list_id = CandidateListId::new();
            let mut list = sample_candidate_list(list_id);
            list.electoral_districts = vec![district];
            list.candidates = vec![person_id];
            store.add_candidate_list(list);
            lists.push(list_id);
        }

        let candidate_omission = |title: &str, lists: Vec<CandidateListId>| {
            Omission::new(
                OmissionCategory::Candidate {
                    person: person_id,
                    lists,
                },
                title.parse().unwrap(),
                "The document is missing.".parse().unwrap(),
                None,
            )
        };
        candidate_omission("Missing consent", lists.clone())
            .create(&store)
            .await
            .unwrap();
        // Reported on the other list only; the page says so.
        candidate_omission("Missing identity document", vec![lists[1]])
            .create(&store)
            .await
            .unwrap();

        let response = candidate(
            CsbRecoveryCandidatePath {
                stream_id,
                list_id: lists[0],
                person_id,
            },
            CsbContext::new_test(),
            store,
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        // The candidate's omissions on every list, each naming its list: one
        // decision per list for the shared one...
        assert!(body.contains("Missing consent"));
        assert_eq!(body.matches(r#"name="candidate_list""#).count(), 2);
        assert!(body.contains(&format!(r#"value="{}""#, lists[0])));
        assert!(body.contains(&format!(r#"value="{}""#, lists[1])));
        assert!(body.contains("1. Groningen"));
        // ...and the other list's own omission decided as a whole, but named.
        assert!(body.contains("Missing identity document"));
        assert!(body.contains("Candidate list"));
        assert_eq!(body.matches("7. Utrecht").count(), 2);
    }

    #[tokio::test]
    async fn recovery_general_information_hides_correction_links() {
        let store = CsbStore::new_for_test();
        store.set_political_group(sample_political_group());
        let stream_id = store.stream_id;

        let response = general_information(
            CsbRecoveryGeneralInformationPath { stream_id },
            CsbContext::new_test(),
            store,
        )
        .await
        .unwrap()
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_string(response).await;
        assert!(body.contains("Kiesraad Demo"));
        assert!(!body.contains("/correction/"));
        assert!(!body.contains("/csb/examination/"));
    }

    #[tokio::test]
    async fn scrapped_appellation_renders() {
        let store = CsbStore::new_for_test();

        // add a name authorisation
        store
            .update(CsbAction::PaperCorrectedUpdate(Box::new(
                PgEvent::CreateNameAuthorisation(sample_name_authorisation(
                    NameAuthorisationId::new(),
                )),
            )))
            .await
            .expect("Create name authorisation");

        // add irrecoverable omission for the appellation
        let mut omission = Omission::new(
            OmissionCategory::Appellation,
            OmissionTitle::from_str("unregistered").unwrap(),
            OmissionText::from_str("unregistered").unwrap(),
            None,
        );
        omission.recoverable = false;
        store
            .update(CsbAction::CreateOmission(omission))
            .await
            .expect("Update store with appellation omission creation");

        let response = general_information(
            CsbRecoveryGeneralInformationPath {
                stream_id: store.stream_id,
            },
            CsbContext::new_test(),
            store,
        )
        .await
        .unwrap()
        .into_response();

        let body = response_body_string(response).await;

        assert!(body.contains("Blank list"));
        // 1 badge for the appellation and 2 badges for the name authorisation
        assert_eq!(body.matches("Scrapped</span>").count(), 3);
    }
}
