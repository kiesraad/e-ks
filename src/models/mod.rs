//! The official election PDF models, rendered in-process with
//! [`textris_pdf`].
//!
//! Each model lives in its own file (`h1`, `h3`, `h4`, `h9`, `i1`, `i4`, plus
//! the omission letter in `omission_letter` and the pre-submission overview in
//! `brp_overview`); H 3 covers both the H 3-1 and H 3-2 variants. The document text is authored as askama
//! Markdown templates in `templates/` (one per locale and variant), written in
//! the textris-pdf Markdown dialect and wired up by [`mod@markdown`].
//! [`layout`] holds the shared page set-up, and [`inputs`] the shared input
//! data types plus their conversions from the application store types.
//! [`mod@examples`] defines type-checked example inputs, rendered by the
//! round-trip test and the `pdf_diff` development tool.
//!
//! [`mod@documents`] collects the store data for a candidate list and streams
//! the rendered models plus the [`mod@eml::eml210`] nomination export as a ZIP
//! download.

pub mod brp_overview;
pub(crate) mod documents;
pub(crate) mod eml;
pub mod examples;
mod fonts;
pub mod h1;
pub mod h3;
pub mod h4;
pub mod h9;
pub mod i1;
pub mod i4;
pub mod inputs;
mod layout;
mod markdown;
pub mod omission_letter;

pub use examples::{Example, examples};
pub use fonts::fonts;

use axum::{
    http::HeaderValue,
    response::{IntoResponse, Response},
};
use textris_pdf::{build::Textris, render::RenderError};

use crate::{AppError, utils::no_cache_headers};

pub const PDF_CONTENT_TYPE: &str = "application/pdf";
pub const DOCX_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document";

/// A document that renders to a PDF: it can build a [`Textris`] document and
/// knows its download file name.
pub trait Pdf: Sized {
    /// Build the document from the input data.
    fn document(&self) -> Result<Textris, AppError>;

    fn filename(&self) -> String;

    /// Render the accessible PDF/A-2A + PDF/UA-1 bytes on a blocking thread
    /// (rendering is CPU-bound).
    // The trait is only consumed inside this crate, so auto trait bounds on
    // the returned future don't need to be nameable.
    #[allow(async_fn_in_trait)]
    async fn generate_bytes(&self) -> Result<Vec<u8>, AppError> {
        let document = self.document()?;
        render_blocking(move || document.render(fonts())).await
    }

    /// [`Self::filename`] with the `.docx` extension.
    fn docx_filename(&self) -> String {
        let filename = self.filename();
        format!(
            "{}.docx",
            filename.strip_suffix(".pdf").unwrap_or(&filename)
        )
    }

    /// Export the Word (`.docx`) bytes on a blocking thread. This is a
    /// structural export of the same document: content and coarse structure,
    /// but not the PDF's styling.
    #[allow(async_fn_in_trait)]
    async fn generate_docx_bytes(&self) -> Result<Vec<u8>, AppError> {
        let document = self.document()?;
        tokio::task::spawn_blocking(move || document.to_docx())
            .await
            .map_err(|_| AppError::InternalServerError)?
            .map_err(AppError::DocxError)
    }

    /// The rendered PDF as a download response under [`Self::filename`].
    #[allow(async_fn_in_trait)]
    async fn pdf_response(&self) -> Result<Response, AppError> {
        let headers = no_cache_headers::generate_attachment_headers(
            &self.filename(),
            HeaderValue::from_static(PDF_CONTENT_TYPE),
        )?;
        Ok((headers, self.generate_bytes().await?).into_response())
    }

    /// The Word export as a download response under [`Self::docx_filename`].
    #[allow(async_fn_in_trait)]
    async fn docx_response(&self) -> Result<Response, AppError> {
        let headers = no_cache_headers::generate_attachment_headers(
            &self.docx_filename(),
            HeaderValue::from_static(DOCX_CONTENT_TYPE),
        )?;
        Ok((headers, self.generate_docx_bytes().await?).into_response())
    }
}

/// Run a PDF render on a blocking thread
pub(crate) async fn render_blocking(
    render: impl FnOnce() -> Result<Vec<u8>, RenderError> + Send + 'static,
) -> Result<Vec<u8>, AppError> {
    Ok(tokio::task::spawn_blocking(render)
        .await
        .map_err(|_| AppError::InternalServerError)??)
}

#[cfg(test)]
mod tests {
    use super::{examples::*, *};
    use crate::core::{ElectionType, ModelLocale};

    #[track_caller]
    fn assert_pdf(bytes: &[u8], ctx: &str) {
        assert!(bytes.starts_with(b"%PDF"), "{ctx}: output is not a PDF");
        assert!(
            bytes.len() > 1000,
            "{ctx}: PDF unexpectedly small ({} bytes)",
            bytes.len()
        );
    }

    #[track_caller]
    fn render<T: Pdf>(model: T) -> Vec<u8> {
        model
            .document()
            .expect("build model document")
            .render(fonts())
            .expect("render model")
    }

    /// Every example input renders to a valid PDF. This drives all nine
    /// document builders (`h1`, `h3-1`, `h3-2`, `h4`, `h9`, `i1`, `i4`, the
    /// omission letter and the pre-submission overview) together with the
    /// shared layout code, end to end.
    #[test]
    fn renders_every_example_input() {
        let mut rendered = 0;
        for example in examples() {
            let name = example.name;
            assert_pdf(&example.render().expect("render example"), name);
            rendered += 1;
        }
        assert_eq!(rendered, 23, "expected to render every example input");
    }

    /// Every example input also exports as a Word document, which exercises the
    /// docx translation of every block type the models use.
    #[test]
    fn exports_every_example_as_docx() {
        for example in examples() {
            let bytes = example.to_docx().expect("export example as docx");
            // A .docx is a ZIP archive.
            assert!(
                bytes.starts_with(b"PK"),
                "{}: output is not a ZIP archive",
                example.name
            );
        }
    }

    /// H 1's attachment checklist branches on the election type; render each so
    /// every branch (including EP and the non-resident electoral college) runs.
    #[test]
    fn h1_attachments_cover_every_election_type() {
        use ElectionType::*;
        for election_type in [Tk, Ek, Gr, Ps, Ws, Ep, Kc, Kcni, Er] {
            let mut input = h1_example_2();
            input.common.election_type = election_type;
            assert_pdf(&render(input), &format!("{election_type:?}"));
        }
    }

    /// H 4 renders the mayor's statement for every election type except the
    /// senate (EK), with wording that depends on who keeps the voter register.
    #[test]
    fn h4_mayor_section_per_election_type() {
        use ElectionType::*;
        for election_type in [Ek, Tk, Gr, Er] {
            let mut input = h4_example_1();
            input.common.election_type = election_type;
            assert_pdf(&render(input), &format!("{election_type:?}"));
        }
    }

    /// H 9's notification section differs for the non-resident electoral
    /// college and when neither a representative nor a postal address is given.
    #[test]
    fn h9_notification_branches() {
        // Neither representative nor postal address: "niet van toepassing".
        let mut input = h9_example_1();
        input.detailed_candidate.postal_address = None;
        input.detailed_candidate.bsn = None;
        assert_pdf(&render(input), "h9 without address");

        // Non-resident electoral college: digital-notification consent.
        let mut input = h9_example_1();
        input.common.election_type = ElectionType::Kcni;
        assert_pdf(&render(input), "h9 KCNI");

        // Needs a representative, but it is None.
        let mut input = h9_example_1();
        input.detailed_candidate.needs_representative = true;
        assert_pdf(&render(input), "h9 missing representative");
    }

    /// Render I 4 with every list section empty so the "geen ..." fallbacks run.
    #[test]
    fn i4_renders_with_empty_sections() {
        let mut input = i4_example_1();
        input.found_omissions.clear();
        input.recovered_omissions.clear();
        input.invalid_lists.clear();
        input.removed_candidates.clear();
        input.removed_appellations.clear();
        input.corrected_appellations.clear();
        input.objections = Vec::new();
        input.response_objections = None;
        assert_pdf(&render(input), "i4 empty sections");
    }

    /// I 1 is downloaded before anything was imported too: render it with both
    /// list sections empty so the "geen verzuimen" fallback and the empty
    /// "Kandidatenlijsten" section run. (`i1_example_2` covers the fallback
    /// with lists present.)
    #[test]
    fn i1_renders_with_empty_sections() {
        let mut input = i1_example_1();
        input.submitted_lists.clear();
        input.found_omissions.clear();
        assert_pdf(&render(input), "i1 empty sections");
    }

    /// Models report a download file name; check the locale- and
    /// designation-dependent ones, plus the Dutch-only I 1 and I 4.
    #[test]
    fn filenames() {
        assert_eq!(
            h3_2_example_1().filename(),
            "h3-2-samengevoegde-aanduiding.pdf"
        );

        let mut frisian = h3_1_example_1();
        frisian.common.locale = ModelLocale::Fry;
        assert_eq!(frisian.filename(), "h3-1-oantsjutting.pdf");

        assert_eq!(i1_example_1().filename(), "i1-proces-verbaal.pdf");
        assert_eq!(i4_example_1().filename(), "i4-proces-verbaal.pdf");

        // The omission letter is stamped with the group it is addressed to and
        // the election; a blank list has no appellation to slugify.
        assert_eq!(
            omission_letter_example_1().filename(),
            "verzuimbrief-kiesraad-demo-ek27.pdf"
        );
        let mut blank = omission_letter_example_1();
        blank.appellation = String::new();
        assert_eq!(blank.filename(), "verzuimbrief-ek27.pdf");

        assert_eq!(
            brp_overview_example_1().filename(),
            "brp-overzicht-kiesraad-demo-ek27.pdf"
        );
        assert_eq!(
            brp_overview_example_1().docx_filename(),
            "brp-overzicht-kiesraad-demo-ek27.docx"
        );
    }
}
