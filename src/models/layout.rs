//! Shared page set-up for the PDF models.

use askama::Template;
use textris_pdf::{
    build::{Text, Textris},
    markdown::ParseOptions,
    theme::{Theme, em},
};

use super::inputs::ModelData;
use crate::{AppError, core::ModelLocale};

fn theme() -> Theme {
    let mut theme = Theme::default();
    theme.font_size.h5 = em(1.0);
    theme.table.inset_y = em(0.2);
    theme.table.fill_in_min_height = em(2.0);
    theme.spacing.heading_above.h3 = em(1.5);
    theme.spacing.heading_above.h4 = em(1.5);
    theme.spacing.heading_above.h5 = em(1.5);
    theme.font_size.chrome = em(0.85);
    // Header baseline 30% of the top margin above the content edge
    theme.page.header_offset = 0.3 * theme.page.margin_y;
    theme
}

/// Build a model document from its Markdown template: the shared [`theme`]
/// plus the rendered template as document body and chrome (the title,
/// language, header and page counter come from the template's front matter).
pub(super) fn markdown_document(template: impl Template) -> Result<Textris, AppError> {
    let mut doc = Textris::with_theme(theme());
    let options = ParseOptions {
        // The models number their `###` sections.
        numbered_heading_levels: vec![3],
        ..ParseOptions::default()
    };
    doc.push_markdown(&template.render()?, &options)?;
    Ok(doc)
}

/// [`markdown_document`] plus the hash footer of the EML the document
/// accompanies, if any.
pub(super) fn eml_hash_document(
    template: impl Template,
    hash: Option<&str>,
) -> Result<Textris, AppError> {
    let mut doc = markdown_document(template)?;
    if let Some(hash) = hash {
        hash_footer(&mut doc, hash);
    }
    Ok(doc)
}

/// The left page footer carrying the hash of the accompanying EML, which OSV
/// asks for on import. The muted labels and mono values have no Markdown
/// dialect syntax, so this stays on the builder API.
fn hash_footer(doc: &mut Textris, sha_hash: &str) {
    doc.footer_left(Text::new().muted("Hash:").normal(" ").mono(sha_hash));
}

/// [`markdown_document`] plus the event version footer that all H models
/// carry; I 1 and I 4 do not.
pub(super) fn h_document(common: &ModelData, template: impl Template) -> Result<Textris, AppError> {
    let mut doc = markdown_document(template)?;
    version_footer(&mut doc, common.locale, common.event_id, &common.sha_hash);
    Ok(doc)
}

/// The left page footer carrying the event version and hash. The muted labels
/// and mono values have no Markdown dialect syntax, so this stays on the
/// builder API.
fn version_footer(doc: &mut Textris, locale: ModelLocale, event_id: usize, sha_hash: &str) {
    let version = match locale {
        ModelLocale::Nl => "Versie:",
        ModelLocale::Fry => "Ferzje:",
    };
    doc.footer_left(
        Text::new()
            .muted(version)
            .normal(" ")
            .mono(event_id.to_string())
            .normal("   ")
            .muted("Hash:")
            .normal(" ")
            .mono(sha_hash),
    );
}
