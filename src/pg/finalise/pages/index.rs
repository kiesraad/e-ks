use askama::Template;
use axum::response::IntoResponse;

use crate::{
    AppError, Context, EventHashPrefix, HtmlTemplate, PgStore,
    core::ModelLocale,
    filters,
    finalise::AllProblems,
    structs::{
        common::{HasSeverity, Severity},
        list_designation::ListDesignation,
        list_submitters::ListSubmitter,
    },
};

use super::FinalisePath;

#[derive(Template)]
#[template(path = "pg/finalise/pages/index.html")]
pub struct IndexTemplate {
    problems: AllProblems,
    download_path_nl: String,
    download_path_fry: String,
    frisian_export_allowed: bool,
    list_designation: Option<ListDesignation>,
    previously_seated: bool,
}

pub async fn index(
    _: FinalisePath,
    context: Context,
    store: PgStore,
) -> Result<impl IntoResponse, AppError> {
    let problems = AllProblems::find_all(&store)?;
    let event_hash = EventHashPrefix::of(&store.current_event_hash());

    Ok(HtmlTemplate(
        IndexTemplate {
            problems,
            download_path_nl: super::DownloadDocumentsPath {
                event_hash,
                locale: ModelLocale::Nl,
            }
            .to_string(),
            download_path_fry: super::DownloadDocumentsPath {
                event_hash,
                locale: ModelLocale::Fry,
            }
            .to_string(),
            frisian_export_allowed: context.election.frisian_export_allowed(),
            list_designation: store.get_political_group().list_designation,
            previously_seated: store.get_political_group().was_previously_seated(),
        },
        context,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Context, ElectionConfig, ElectoralDistrict, Locale, PgStore, Session,
        structs::{
            candidate_lists::CandidateListId, list_submitters::ListSubmitterId, persons::PersonId,
        },
        test_utils::{
            response_body_string, sample_candidate_list, sample_list_submitter, sample_person,
        },
    };
    use axum::response::IntoResponse;

    #[tokio::test]
    async fn index_shows_document_downloads_for_complete_lists() -> Result<(), AppError> {
        let store = PgStore::new_for_test();
        let complete_list_id = CandidateListId::new();
        let person_id = PersonId::new();

        sample_list_submitter(ListSubmitterId::new())
            .update(&store)
            .await?;
        sample_person(person_id).create(&store).await?;

        let mut complete_list = sample_candidate_list(complete_list_id);
        complete_list.create(&store).await?;
        complete_list.append_candidate(&store, person_id).await?;

        let event_hash = EventHashPrefix::of(&store.current_event_hash());
        let response = index(FinalisePath, Context::new_test_without_db(), store)
            .await?
            .into_response();
        let body = response_body_string(response).await;

        assert!(
            body.contains(
                &super::super::DownloadDocumentsPath {
                    event_hash,
                    locale: ModelLocale::Nl,
                }
                .to_string()
            )
        );

        assert!(
            body.matches(
                &super::super::DownloadDocumentsPath {
                    event_hash,
                    locale: ModelLocale::Nl,
                }
                .to_string()
            )
            .count()
                == 1
        );

        Ok(())
    }

    #[tokio::test]
    async fn index_shows_nl_and_fry_downloads_when_needed() -> Result<(), AppError> {
        for (election, district) in [
            (
                ElectionConfig::PS27(crate::Province::Fryslan),
                ElectoralDistrict::Fryslan,
            ),
            (
                ElectionConfig::WS27(crate::WaterCouncil::Fryslan),
                ElectoralDistrict::WsFryslan,
            ),
        ] {
            let store = PgStore::new_for_test_with_election(election);
            let complete_list_id = CandidateListId::new();
            let person_id = PersonId::new();

            sample_list_submitter(ListSubmitterId::new())
                .update(&store)
                .await?;
            sample_person(person_id).create(&store).await?;

            let mut complete_list = sample_candidate_list(complete_list_id);
            complete_list.electoral_districts = vec![district];
            complete_list.create(&store).await?;
            complete_list.append_candidate(&store, person_id).await?;

            let event_hash = EventHashPrefix::of(&store.current_event_hash());
            let response = index(
                FinalisePath,
                Context::new(&store, Session::new_test_with_locale(Locale::Nl)),
                store,
            )
            .await?
            .into_response();
            let body = response_body_string(response).await;

            assert!(
                body.contains(
                    &super::super::DownloadDocumentsPath {
                        event_hash,
                        locale: ModelLocale::Nl,
                    }
                    .to_string()
                )
            );

            assert!(
                body.contains(
                    &super::super::DownloadDocumentsPath {
                        event_hash,
                        locale: ModelLocale::Fry,
                    }
                    .to_string()
                ),
                "Expected Frisian link for {:?}\n{}",
                election,
                body
            );
        }

        Ok(())
    }

    #[tokio::test]
    async fn index_shows_only_nl_when_needed() -> Result<(), AppError> {
        for (election, district) in [
            (ElectionConfig::EK27, ElectoralDistrict::Fryslan),
            (
                ElectionConfig::PS27(crate::Province::Groningen),
                ElectoralDistrict::Groningen,
            ),
            (
                ElectionConfig::WS27(crate::WaterCouncil::Noorderzijlvest),
                ElectoralDistrict::WsNoorderzijlvest,
            ),
        ] {
            let store = PgStore::new_for_test_with_election(election);
            let complete_list_id = CandidateListId::new();
            let person_id = PersonId::new();

            sample_list_submitter(ListSubmitterId::new())
                .update(&store)
                .await?;
            sample_person(person_id).create(&store).await?;

            let mut complete_list = sample_candidate_list(complete_list_id);
            complete_list.electoral_districts = vec![district];
            complete_list.create(&store).await?;
            complete_list.append_candidate(&store, person_id).await?;

            let event_hash = EventHashPrefix::of(&store.current_event_hash());
            let response = index(
                FinalisePath,
                Context::new(&store, Session::new_test_with_locale(Locale::Nl)),
                store,
            )
            .await?
            .into_response();
            let body = response_body_string(response).await;

            assert!(
                body.contains(
                    &super::super::DownloadDocumentsPath {
                        event_hash,
                        locale: ModelLocale::Nl,
                    }
                    .to_string()
                )
            );

            assert!(
                !body.contains(
                    &super::super::DownloadDocumentsPath {
                        event_hash,
                        locale: ModelLocale::Fry,
                    }
                    .to_string()
                ),
                "Expected no Frisian link for {:?}\n{}",
                election,
                body
            );
        }

        Ok(())
    }
}
