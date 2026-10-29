use axum::{
    body::Bytes,
    extract::{FromRequest, Multipart, Request},
};

use crate::AppError;

const MAX_FILE_NAME_LEN: usize = 255;

#[derive(Debug, Default)]
pub struct FileForm {
    pub file_name: Option<String>,
    pub file_data: Option<Bytes>,
}

impl<S> FromRequest<S> for FileForm
where
    S: Sync + Send,
{
    type Rejection = AppError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let mut multipart = Multipart::from_request(req, state).await?;
        let mut form = FileForm::default();

        while let Some(field) = multipart.next_field().await? {
            if field.name() == Some("file_data") {
                let content_type = field.content_type().map(ToString::to_string);
                form.file_name = sanitize_file_name(field.file_name());
                let bytes = field.bytes().await?;
                tracing::info!(
                    content_type = content_type.as_deref().unwrap_or(""),
                    size_bytes = bytes.len(),
                    "file upload received",
                );
                form.file_data = Some(bytes);
            }
        }

        Ok(form)
    }
}

/// Strip path components and control characters from an uploaded file name,
/// and truncate to a bounded length. Returns `None` if the result is empty.
fn sanitize_file_name(raw: Option<&str>) -> Option<String> {
    let basename = raw?.rsplit(['/', '\\']).next().unwrap_or_default();

    let cleaned: String = basename
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_FILE_NAME_LEN)
        .collect();

    let trimmed = cleaned.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, extract::FromRequest, http::Request};

    fn multipart_request(path: &str, csv: &str, boundary: &str) -> Request<Body> {
        Request::builder()
            .uri(path)
            .header(
                "Content-Type",
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(Body::from(format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"file_data\"; filename=\"candidates.csv\"\r\nContent-Type: text/csv\r\n\r\n{csv}\r\n--{boundary}--\r\n"
            )))
            .unwrap()
    }

    #[tokio::test]
    async fn extracts_uploaded_file() -> Result<(), AppError> {
        let request = multipart_request(
            "/candidate-lists/import",
            "voorletters,achternaam\r\nJ.,Berg",
            "----eks-boundary",
        );

        let form = FileForm::from_request(request, &()).await?;

        assert_eq!(form.file_name.as_deref(), Some("candidates.csv"));
        assert_eq!(
            form.file_data.as_deref(),
            Some(&b"voorletters,achternaam\r\nJ.,Berg"[..])
        );

        Ok(())
    }

    #[test]
    fn sanitize_file_name_strips_paths_and_control_chars() {
        assert_eq!(
            sanitize_file_name(Some("../../etc/passwd")).as_deref(),
            Some("passwd")
        );
        assert_eq!(
            sanitize_file_name(Some(r"C:\Users\x\report.csv")).as_deref(),
            Some("report.csv")
        );
        assert_eq!(
            sanitize_file_name(Some("bad\x00name\n.csv")).as_deref(),
            Some("badname.csv")
        );
        assert_eq!(sanitize_file_name(Some("   ")), None);
        assert_eq!(sanitize_file_name(Some("")), None);
        assert_eq!(sanitize_file_name(None), None);
    }

    #[test]
    fn sanitize_file_name_truncates_to_max_length() {
        let long = "a".repeat(500) + ".csv";
        let sanitized = sanitize_file_name(Some(&long)).unwrap();
        assert_eq!(sanitized.chars().count(), MAX_FILE_NAME_LEN);
    }
}
