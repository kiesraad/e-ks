use askama::Template;
use axum::{
    extract::OriginalUri,
    http::StatusCode,
    response::{IntoResponse, Response},
};

use crate::{Context, CsbContext, HtmlTemplate, csb::common::CsbNotFoundPath, filters};

#[derive(Template)]
#[template(path = "csb/common/pages/not_found.html")]
struct CsbNotFoundTemplate {
    path: String,
}

/// The CSB not-found page, for paths under `/csb` that no route claims.
pub async fn not_found(
    _: CsbNotFoundPath,
    OriginalUri(uri): OriginalUri,
    context: CsbContext,
) -> Response {
    let template = CsbNotFoundTemplate {
        path: uri.to_string(),
    };

    (StatusCode::NOT_FOUND, HtmlTemplate(template, context)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_utils::response_body_string;

    #[tokio::test]
    async fn not_found_renders_csb_layout_with_path() {
        let response = not_found(
            CsbNotFoundPath {
                path: "missing".to_string(),
            },
            OriginalUri("/csb/missing".parse().unwrap()),
            CsbContext::new_test(),
        )
        .await;

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = response_body_string(response).await;
        assert!(body.contains("The page you are looking for does not exist"));
        assert!(body.contains("/csb/missing"));
        // The overview link leads back into the CSB section.
        assert!(body.contains("href=\"/csb\""));
    }
}
