use axum::{http::HeaderValue, response::IntoResponse};

use crate::{
    AppError, CsbMainStore, csb::index::CsbElectionDefinitionDownloadPath,
    models::eml::eml110a::eml110a, utils::no_cache_headers,
};

const XML_CONTENT_TYPE: &str = "application/xml";

pub async fn download_election_definition(
    _: CsbElectionDefinitionDownloadPath,
    main_store: CsbMainStore,
) -> Result<impl IntoResponse, AppError> {
    let bytes = eml110a(
        &main_store.election,
        &main_store.registered_political_groups(),
    )?;

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
        body::to_bytes,
        http::{StatusCode, header},
        response::IntoResponse,
    };

    use crate::{CsbMainAction, CsbUser, structs::csb::sample_registered_political_group};

    #[tokio::test]
    async fn download_election_definition_returns_xml_response() -> Result<(), AppError> {
        let main_store = CsbMainStore::new_for_test();
        for group in [
            sample_registered_political_group("Kleine Partij", 10, 0),
            sample_registered_political_group("Grote Partij", 1000, 5),
        ] {
            main_store
                .update(
                    CsbMainAction::CreateRegisteredPoliticalGroup(group).by(CsbUser::new_test()),
                )
                .await?;
        }

        let response = download_election_definition(CsbElectionDefinitionDownloadPath, main_store)
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

        // the registered groups are exported, most votes first
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let xml = String::from_utf8(body.to_vec()).expect("valid utf-8");
        let position = |appellation| xml.find(appellation).expect("registered appellation");
        assert!(position("Grote Partij") < position("Kleine Partij"));

        Ok(())
    }
}
