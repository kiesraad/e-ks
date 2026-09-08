use axum::{extract::State, http::HeaderValue, response::IntoResponse};

use crate::{
    AppError, AppRequestState, CsbMainStore, csb::index::CsbElectionDefinitionDownloadPath,
    models::eml::eml110a::eml110a, projection::WithCorrections, utils::no_cache_headers,
};

const XML_CONTENT_TYPE: &str = "application/xml";

pub async fn download_election_definition<S: AppRequestState>(
    _: CsbElectionDefinitionDownloadPath,
    main_store: CsbMainStore,
    State(state): State<S>,
) -> Result<impl IntoResponse, AppError> {
    let csb_registry = state.csb_store_registry();
    let election = main_store.election;

    // TODO: get registered party from a proper source
    // Blank lists probably shouldn't be included
    let registered_party_names = csb_registry
        .stores_for_election(election)
        .await?
        .into_iter()
        .map(|store| store.get_appellation(WithCorrections::All))
        .collect();

    let bytes = eml110a(&election, registered_party_names)?;

    let headers = no_cache_headers::generate_attachment_headers(
        "eml110a.eml.xml",
        HeaderValue::from_static(XML_CONTENT_TYPE),
    )?;

    Ok((headers, bytes).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        http::{StatusCode, header},
        response::IntoResponse,
    };

    use crate::AppState;

    #[tokio::test]
    async fn download_election_definition_returns_xml_response() -> Result<(), AppError> {
        let main_store = CsbMainStore::new_for_test();
        let state = AppState::new_for_tests().await;

        let response = download_election_definition(
            CsbElectionDefinitionDownloadPath,
            main_store,
            State(state),
        )
        .await?
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let headers = response.headers();
        assert_eq!(
            headers.get(header::CONTENT_TYPE).expect("content type"),
            "application/xml"
        );
        assert_eq!(
            headers
                .get(header::CONTENT_DISPOSITION)
                .expect("content disposition"),
            "attachment; filename=\"eml110a.eml.xml\""
        );
        assert_eq!(
            headers.get(header::CACHE_CONTROL).expect("cache control"),
            "no-store, no-cache, must-revalidate, max-age=0"
        );

        Ok(())
    }
}
