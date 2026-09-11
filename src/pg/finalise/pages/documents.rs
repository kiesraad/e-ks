use axum::response::IntoResponse;

use crate::{
    AppError, Context, PgStore,
    finalise::{AllProblems, pages::DownloadDocumentsPath},
    models::documents::DocumentData,
};

pub async fn gen_documents(
    path @ DownloadDocumentsPath { locale }: DownloadDocumentsPath,
    store: PgStore,
    context: Context,
) -> Result<impl IntoResponse, AppError> {
    // same gate as the finalise page, before generating anything
    if !AllProblems::find_all(&store)?.models_downloadable() {
        return Err(AppError::NotDownloadable);
    }

    let (bundles, filename) = DocumentData::from_store_and_context(&store, &context, locale)?;

    DocumentData::serve_download(bundles, filename, path.to_string(), &store, &store).await
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;

    use super::*;
    use crate::{
        ElectionConfig,
        core::ModelLocale,
        structs::{
            candidate_lists::CandidateList,
            common::{BsnOrNoneConfirmed, CountryCode, FullName},
            name_authorisations::NameAuthorisationId,
            persons::Representative,
        },
        test_utils::{sample_name_authorisation, setup_documents_test_state},
    };

    #[tokio::test]
    async fn gen_documents_missing_list_submitter_returns_error() -> Result<(), AppError> {
        let (store, _, context) =
            setup_documents_test_state(1, 1, false, true, ElectionConfig::EK27).await?;
        let result = gen_documents(
            DownloadDocumentsPath {
                locale: crate::core::ModelLocale::Nl,
            },
            store,
            context,
        )
        .await;

        assert!(matches!(result, Err(AppError::NotDownloadable)));

        Ok(())
    }

    #[tokio::test]
    async fn gen_documents_multiple_name_authorisations_return_error() -> Result<(), AppError> {
        let (store, _, context) =
            setup_documents_test_state(1, 1, true, true, ElectionConfig::EK27).await?;
        sample_name_authorisation(NameAuthorisationId::new())
            .create(&store)
            .await?;
        let result = gen_documents(
            DownloadDocumentsPath {
                locale: crate::core::ModelLocale::Nl,
            },
            store.clone(),
            context.clone(),
        )
        .await;

        // the gate refuses before document generation can
        assert!(matches!(result, Err(AppError::NotDownloadable)));
        match DocumentData::from_store_and_context(&store, &context, ModelLocale::Nl) {
            Err(AppError::IncompleteData(message)) => {
                assert_eq!(message, "Expected no more than 1 name authorisation")
            }
            _ => panic!("expected IncompleteData error"),
        }

        Ok(())
    }

    #[tokio::test]
    async fn multiple_name_authorisations_ok_for_list_combinations() -> Result<(), AppError> {
        use axum::response::IntoResponse;

        let (store, _, context) =
            setup_documents_test_state(1, 1, true, true, ElectionConfig::EK27).await?;

        let mut political_group = store.get_political_group();
        political_group.list_designation =
            Some(crate::structs::list_designation::ListDesignation::Combined);
        political_group.update(&store).await?;

        let response = gen_documents(
            DownloadDocumentsPath {
                locale: crate::core::ModelLocale::Nl,
            },
            store,
            context,
        )
        .await?
        .into_response();

        let entry_names = crate::test_utils::zip_entry_names(response).await;
        assert!(entry_names.contains(&"h3-2-samengevoegde-aanduiding.pdf".to_string()));

        Ok(())
    }

    #[tokio::test]
    async fn blank_lists_produce_no_h3() -> Result<(), AppError> {
        use axum::response::IntoResponse;

        let (store, _, context) =
            setup_documents_test_state(1, 1, true, true, ElectionConfig::EK27).await?;

        let mut political_group = store.get_political_group();
        political_group.list_designation =
            Some(crate::structs::list_designation::ListDesignation::Blank);
        political_group.update(&store).await?;

        let response = gen_documents(
            DownloadDocumentsPath {
                locale: crate::core::ModelLocale::Nl,
            },
            store,
            context,
        )
        .await?
        .into_response();

        let entry_names = crate::test_utils::zip_entry_names(response).await;
        assert!(!entry_names.iter().any(|name| name.starts_with("h3")));

        Ok(())
    }

    #[tokio::test]
    async fn gen_documents_missing_designation_returns_error() -> Result<(), AppError> {
        let (store, _, context) =
            setup_documents_test_state(1, 1, true, true, ElectionConfig::EK27).await?;

        let mut political_group = store.get_political_group();
        political_group.appellation = None;
        political_group.update(&store).await?;

        let result = gen_documents(
            DownloadDocumentsPath {
                locale: crate::core::ModelLocale::Nl,
            },
            store,
            context,
        )
        .await;

        assert!(matches!(result, Err(AppError::NotDownloadable)));

        Ok(())
    }

    #[tokio::test]
    async fn gen_documents_disallowed_frisian_export_returns_error() -> Result<(), AppError> {
        let (store, _, context) = setup_documents_test_state(
            1,
            1,
            true,
            true,
            ElectionConfig::PS27(crate::Province::Groningen),
        )
        .await?;

        let result = gen_documents(
            DownloadDocumentsPath {
                locale: ModelLocale::Fry,
            },
            store,
            context,
        )
        .await;

        match result {
            Err(AppError::UserError(message)) => {
                assert_eq!(message, "Frisian export not allowed for this election")
            }
            _ => panic!("expected disallowed Frisian export error"),
        }

        Ok(())
    }

    /// The download rate limit is enforced before any PDF is rendered: the
    /// download event is recorded first, and the render only starts once that
    /// event is accepted.
    #[tokio::test]
    async fn gen_documents_refuses_a_submission_with_errors() -> Result<(), AppError> {
        let (store, list_ids, context) =
            setup_documents_test_state(2, 2, true, true, ElectionConfig::EK27).await?;

        // duplicate districts: an error document generation itself does not catch
        let first = store.get_candidate_list(list_ids[0])?;
        let second = CandidateList {
            electoral_districts: first.electoral_districts.clone(),
            ..store.get_candidate_list(list_ids[1])?
        };
        second.update_districts(&store).await?;
        assert!(!AllProblems::find_all(&store)?.models_downloadable());

        match gen_documents(
            DownloadDocumentsPath {
                locale: ModelLocale::Nl,
            },
            store.clone(),
            context,
        )
        .await
        {
            Err(AppError::NotDownloadable) => {}
            Err(err) => panic!("expected the download gate, got {err:?}"),
            Ok(_) => panic!("download must be refused"),
        }
        assert!(
            !store
                .get_events()
                .iter()
                .any(|e| matches!(e.payload, crate::PgEvent::DownloadFile { .. })),
            "a refused download is not recorded"
        );

        Ok(())
    }

    #[tokio::test]
    async fn gen_documents_is_rate_limited() -> Result<(), AppError> {
        let (store, _, context) =
            setup_documents_test_state(1, 1, true, true, ElectionConfig::EK27).await?;
        let limits = crate::RateLimits::new_for_test(1, 0, 0, TimeDelta::minutes(1));
        let store = store.with_limits(limits);
        let path = || DownloadDocumentsPath {
            locale: ModelLocale::Nl,
        };

        gen_documents(path(), store.clone(), context.clone()).await?;

        match gen_documents(path(), store, context).await {
            Err(err @ AppError::TooManyDownloads { max: 1, .. }) => assert_eq!(
                err.to_string(),
                "Rate limit reached: at most 1 downloads per 60 seconds"
            ),
            Err(err) => panic!("expected a download rate limit error, got {err:?}"),
            Ok(_) => panic!("second download must be refused"),
        }

        Ok(())
    }

    #[tokio::test]
    async fn gen_documents_returns_zip_response() -> Result<(), AppError> {
        use axum::{http::StatusCode, response::IntoResponse};

        use crate::test_utils::assert_zip_response_headers;

        let (store, list_ids, context) =
            setup_documents_test_state(2, 2, true, true, ElectionConfig::EK27).await?;
        let expected_folders = list_ids
            .iter()
            .map(|&list_id| {
                DocumentData::new(&store, &context, list_id, ModelLocale::Nl)
                    .map(|bundle| bundle.folder_name.expect("folder name"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let response = gen_documents(
            DownloadDocumentsPath {
                locale: crate::core::ModelLocale::Nl,
            },
            store,
            context,
        )
        .await?
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        assert_zip_response_headers(response.headers());

        let entry_names = crate::test_utils::zip_entry_names(response).await;
        for folder in expected_folders {
            assert!(entry_names.contains(&format!("{folder}/eml210.eml.xml")));
            assert!(
                entry_names
                    .iter()
                    .any(|name| name == &format!("{folder}/h1-kandidatenlijst.pdf"))
            );
            assert!(
                entry_names
                    .iter()
                    .any(|name| name == &format!("{folder}/h3-1-aanduiding.pdf"))
            );
            assert!(
                entry_names
                    .iter()
                    .any(|name| name == &format!("{folder}/h4-ondersteuningsverklaring.pdf"))
            );
            assert_eq!(
                entry_names
                    .iter()
                    .filter(|name| {
                        name.starts_with(&format!("{folder}/h9-instemmingsverklaringen/"))
                    })
                    .count(),
                2
            );
        }

        Ok(())
    }

    #[tokio::test]
    async fn gen_documents_single_list_writes_files_at_zip_root() -> Result<(), AppError> {
        use axum::response::IntoResponse;

        let (store, _, context) =
            setup_documents_test_state(1, 2, true, true, ElectionConfig::EK27).await?;
        let response = gen_documents(
            DownloadDocumentsPath {
                locale: crate::core::ModelLocale::Nl,
            },
            store,
            context,
        )
        .await?
        .into_response();

        let entry_names = crate::test_utils::zip_entry_names(response).await;
        assert!(entry_names.contains(&"eml210.eml.xml".to_string()));
        assert!(entry_names.contains(&"h1-kandidatenlijst.pdf".to_string()));
        assert!(entry_names.contains(&"h3-1-aanduiding.pdf".to_string()));
        assert!(entry_names.contains(&"h4-ondersteuningsverklaring.pdf".to_string()));
        assert_eq!(
            entry_names
                .iter()
                .filter(|name| name.starts_with("h9-instemmingsverklaringen/"))
                .count(),
            2
        );
        assert!(
            entry_names
                .iter()
                .all(|name| !name.starts_with("documents-")),
            "did not expect a folder prefix for a single list: {entry_names:?}"
        );

        Ok(())
    }

    #[tokio::test]
    async fn gen_documents_single_list_allows_candidate_warnings() -> Result<(), AppError> {
        use axum::response::IntoResponse;

        let (store, list_ids, context) =
            setup_documents_test_state(1, 2, true, true, ElectionConfig::EK27).await?;
        let list = store.get_candidate_list(list_ids[0])?;

        let mut dutch_candidate = store.get_person(list.candidates[0])?;
        dutch_candidate.address.street_name = None;
        dutch_candidate.address.postal_code = None;
        dutch_candidate.address.locality = None;
        dutch_candidate.personal_data.bsn = None;
        dutch_candidate.update(&store).await?;

        let mut international_candidate = store.get_person(list.candidates[1])?;
        international_candidate.personal_data.country = Some("BE".parse::<CountryCode>().unwrap());
        international_candidate.personal_data.bsn = Some(BsnOrNoneConfirmed::NoneConfirmed);
        international_candidate.representative = Some(Representative::default());
        international_candidate.update(&store).await?;

        let response = gen_documents(
            DownloadDocumentsPath {
                locale: crate::core::ModelLocale::Nl,
            },
            store,
            context,
        )
        .await?
        .into_response();

        let entry_names = crate::test_utils::zip_entry_names(response).await;
        assert!(entry_names.contains(&"eml210.eml.xml".to_string()));
        assert!(entry_names.contains(&"h1-kandidatenlijst.pdf".to_string()));
        assert!(entry_names.contains(&"h3-1-aanduiding.pdf".to_string()));
        assert!(entry_names.contains(&"h4-ondersteuningsverklaring.pdf".to_string()));
        assert_eq!(
            entry_names
                .iter()
                .filter(|name| name.starts_with("h9-instemmingsverklaringen/"))
                .count(),
            2
        );

        Ok(())
    }

    #[tokio::test]
    async fn gen_documents_single_list_allows_general_information_warnings() -> Result<(), AppError>
    {
        use axum::response::IntoResponse;

        let (store, _, context) =
            setup_documents_test_state(1, 1, true, true, ElectionConfig::EK27).await?;

        let mut name_auth = store.get_name_authorisations().remove(0);
        name_auth.name = FullName::default();
        name_auth.legal_name = Default::default();
        name_auth.update(&store).await?;

        let response = gen_documents(
            DownloadDocumentsPath {
                locale: crate::core::ModelLocale::Nl,
            },
            store,
            context,
        )
        .await?
        .into_response();

        let entry_names = crate::test_utils::zip_entry_names(response).await;
        assert!(entry_names.contains(&"eml210.eml.xml".to_string()));
        assert!(entry_names.contains(&"h1-kandidatenlijst.pdf".to_string()));
        assert!(entry_names.contains(&"h3-1-aanduiding.pdf".to_string()));
        assert!(entry_names.contains(&"h4-ondersteuningsverklaring.pdf".to_string()));
        assert_eq!(
            entry_names
                .iter()
                .filter(|name| name.starts_with("h9-instemmingsverklaringen/"))
                .count(),
            1
        );

        Ok(())
    }
}
