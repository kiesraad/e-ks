use axum::{
    body::Bytes,
    extract::{FromRequest, Multipart, Request},
};

use crate::{AppError, structs::common::FileName};

#[derive(Debug, Default)]
pub struct FileForm {
    pub file_name: Option<FileName>,
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
                form.file_name = field.file_name().and_then(FileName::from_upload);
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

        assert_eq!(form.file_name, FileName::from_upload("candidates.csv"));
        assert_eq!(
            form.file_data.as_deref(),
            Some(&b"voorletters,achternaam\r\nJ.,Berg"[..])
        );

        Ok(())
    }
}
