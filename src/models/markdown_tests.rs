//! Property tests for the Markdown escaping of the PDF model templates: an
//! interpolated value never changes the document structure, and reads back
//! as the normalisation documented in `textris_pdf::markdown::escape`.
//!
//! Each context is a small inline `.md` template, so the production escaper
//! wiring (`askama.toml`) and filters are what get tested, parsed the way the
//! models are ([`markdown_document`]).

use askama::Template;
use proptest::prelude::*;
use textris_pdf::model::{Block, Cell, Inline, plain_text};

use super::{layout::markdown_document, markdown::filters};
use crate::test_support::strategies::{
    appellation, dangerous_string, hostile_string, initials, last_name, locality,
};

/// A benign value to compare the structure against.
const PLACEHOLDER: &str = "Placeholder";

/// Drop control characters other than `\n`, collapse each whitespace run that
/// holds a `\n` to `line_break`, and drop trailing whitespace.
fn fold(value: &str, line_break: &str) -> String {
    let chars: Vec<char> = value
        .chars()
        .filter(|&c| c == '\n' || !c.is_control())
        .collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if !chars[i].is_whitespace() {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i == chars.len() {
            break;
        }
        let run = &chars[start..i];
        if run.contains(&'\n') {
            out.push_str(line_break);
        } else {
            out.extend(run);
        }
    }
    out
}

/// A value alone in a flow block: hard breaks, trimmed at the block edges.
fn flow_block(value: &str) -> String {
    fold(value, "\n").trim().to_string()
}

/// A value inside a paragraph line, between fixed text.
fn flow_inline(value: &str) -> String {
    format!("Label: {} (end)", fold(value, "\n"))
}

/// The `line` filter: newlines fold to spaces.
fn line(value: &str) -> String {
    fold(&value.replace(['\r', '\n'], " "), "\n")
        .trim()
        .to_string()
}

/// A table cell: the row splitter trims the cell, but a leading `<br>` stays.
fn cell(value: &str) -> String {
    let folded = fold(value, "\n");
    if folded.starts_with('\n') {
        folded
    } else {
        folded.trim_start().to_string()
    }
}

/// A verbatim span: newline runs fold to a space, leading spaces stay.
fn mono(value: &str) -> String {
    fold(value, " ")
}

/// `Label: **value**`, as the models emphasise data.
fn bold_inline(value: &str) -> String {
    format!("Label: {}", fold(value, "\n"))
}

/// `#### 1. value`, a flow-escaped heading.
fn numbered_heading(value: &str) -> String {
    format!("1. {}", fold(value, "\n")).trim().to_string()
}

/// One inline template per context, with the value in `v`.
macro_rules! md_template {
    ($name:ident, $source:literal) => {
        #[derive(Template)]
        #[template(ext = "md", source = $source)]
        struct $name<'a> {
            v: &'a str,
        }
    };
}

md_template!(ParagraphMd, "Intro.\n\n{{ v }}\n\nOutro.\n");
md_template!(InlineMd, "Intro.\n\nLabel: {{ v }} (end)\n\nOutro.\n");
md_template!(BulletMd, "- First\n- {{ v }}\n- Last\n\nOutro.\n");
md_template!(LetteredMd, "a. First\nb. {{ v }}\nc. Last\n\nOutro.\n");
md_template!(QuoteMd, "Intro.\n\n> {{ v }}\n\nOutro.\n");
md_template!(HeadingMd, "Intro.\n\n### {{ v|line }}\n\nOutro.\n");
md_template!(
    CellMd,
    "| Name | Value |\n| --- | --- |\n| x | {{ v|cell }} |\n| y | z |\n\nOutro.\n"
);
md_template!(
    MonoCellMd,
    "| Name | Value |\n| --- | --- |\n| x | {{ v|mono_cell }} |\n| y | z |\n\nOutro.\n"
);
md_template!(BoldMd, "Intro.\n\nLabel: **{{ v }}**\n\nOutro.\n");
md_template!(FlowHeadingMd, "Intro.\n\n#### 1. {{ v }}\n\nOutro.\n");
md_template!(
    BoldCellMd,
    "| a | b |\n| --- | --- |\n| **1** | **{{ v }}** |\n\nOutro.\n"
);

/// The body blocks of a rendered template, or `None` on a render error.
fn blocks(template: impl Template) -> Option<Vec<Block>> {
    markdown_document(template)
        .ok()
        .map(|doc| doc.document().blocks.clone())
}

/// The run styles of some inlines, text erased.
fn runs(inlines: &[Inline]) -> String {
    inlines
        .iter()
        .map(|i| {
            format!(
                "[b{} i{} m{} c{:?} r{} f{:?}]",
                i.bold,
                i.italic,
                i.mono,
                i.color,
                i.section_ref.is_some(),
                i.fill_in
            )
        })
        .collect()
}

fn cell_shape(cell: &Cell) -> String {
    match cell {
        Cell::Text(inlines) => format!("text{}", runs(inlines)),
        Cell::FillIn => "fill-in".to_string(),
        Cell::Blank => "blank".to_string(),
        Cell::Spacer(height) => format!("spacer {height}"),
    }
}

/// Block structure with all text erased: kinds, counts, flags, run styles.
fn shape(blocks: &[Block]) -> Vec<String> {
    blocks
        .iter()
        .map(|block| match block {
            Block::Heading {
                level,
                content,
                numbered,
                label,
            } => format!("h{level} {numbered} {label:?} {}", runs(content)),
            Block::Paragraph(content) => format!("p {}", runs(content)),
            Block::Table(table) => format!(
                "table {:?} {:?} {:?}",
                table.style,
                table.headers.iter().map(cell_shape).collect::<Vec<_>>(),
                table
                    .rows
                    .iter()
                    .map(|row| row.iter().map(cell_shape).collect::<Vec<_>>())
                    .collect::<Vec<_>>()
            ),
            Block::TaskList(items) => format!(
                "tasks {:?}",
                items
                    .iter()
                    .map(|item| (item.checked, runs(&item.content)))
                    .collect::<Vec<_>>()
            ),
            Block::BulletList(items) => {
                format!("ul {:?}", items.iter().map(|i| runs(i)).collect::<Vec<_>>())
            }
            Block::OrderedList { marker, items } => format!(
                "ol {marker:?} {:?}",
                items.iter().map(|i| runs(i)).collect::<Vec<_>>()
            ),
            Block::Box { style, content } => format!("box {style:?} {:?}", shape(content)),
            Block::PageBreak => "page-break".to_string(),
            Block::Spacer(height) => format!("spacer {height}"),
        })
        .collect()
}

fn second_paragraph(blocks: &[Block]) -> Option<&[Inline]> {
    match blocks.get(1)? {
        Block::Paragraph(content) => Some(content),
        _ => None,
    }
}

fn second_item(blocks: &[Block]) -> Option<&[Inline]> {
    match blocks.first()? {
        Block::BulletList(items) | Block::OrderedList { items, .. } => {
            items.get(1).map(Vec::as_slice)
        }
        _ => None,
    }
}

fn quoted(blocks: &[Block]) -> Option<&[Inline]> {
    match blocks.get(1)? {
        Block::Box { content, .. } => match content.first()? {
            Block::Paragraph(inlines) => Some(inlines),
            _ => None,
        },
        _ => None,
    }
}

fn second_heading(blocks: &[Block]) -> Option<&[Inline]> {
    match blocks.get(1)? {
        Block::Heading { content, .. } => Some(content),
        _ => None,
    }
}

fn value_cell(blocks: &[Block]) -> Option<&[Inline]> {
    match blocks.first()? {
        Block::Table(table) => Some(table.rows.first()?.get(1)?.inlines()),
        _ => None,
    }
}

/// One interpolation context: how to render a value into it, where its text
/// lands, and what that text should be.
struct Context {
    render: fn(&str) -> Option<Vec<Block>>,
    target: fn(&[Block]) -> Option<&[Inline]>,
    expected: fn(&str) -> String,
}

const PARAGRAPH: Context = Context {
    render: |v| blocks(ParagraphMd { v }),
    target: second_paragraph,
    expected: flow_block,
};
const INLINE: Context = Context {
    render: |v| blocks(InlineMd { v }),
    target: second_paragraph,
    expected: flow_inline,
};
const BULLET: Context = Context {
    render: |v| blocks(BulletMd { v }),
    target: second_item,
    expected: flow_block,
};
const LETTERED: Context = Context {
    render: |v| blocks(LetteredMd { v }),
    target: second_item,
    expected: flow_block,
};
const QUOTE: Context = Context {
    render: |v| blocks(QuoteMd { v }),
    target: quoted,
    expected: flow_block,
};
const HEADING: Context = Context {
    render: |v| blocks(HeadingMd { v }),
    target: second_heading,
    expected: line,
};
const CELL: Context = Context {
    render: |v| blocks(CellMd { v }),
    target: value_cell,
    expected: cell,
};
const MONO_CELL: Context = Context {
    render: |v| blocks(MonoCellMd { v }),
    target: value_cell,
    expected: mono,
};
const BOLD: Context = Context {
    render: |v| blocks(BoldMd { v }),
    target: second_paragraph,
    expected: bold_inline,
};
const FLOW_HEADING: Context = Context {
    render: |v| blocks(FlowHeadingMd { v }),
    target: second_heading,
    expected: numbered_heading,
};
const BOLD_CELL: Context = Context {
    render: |v| blocks(BoldCellMd { v }),
    target: value_cell,
    expected: |v| fold(v, "\n"),
};

/// Rendering `value` gives the structure of rendering [`PLACEHOLDER`] (or the
/// empty string, for a value that normalises away), and the value's text is
/// its normalisation.
fn check(context: &Context, value: &str) -> Result<(), TestCaseError> {
    let empty = fold(value, "\n").is_empty();
    let placeholder = if empty { "" } else { PLACEHOLDER };
    let reference = (context.render)(placeholder);
    prop_assert!(empty || reference.is_some());
    let actual = (context.render)(value);
    prop_assert_eq!(
        actual.as_deref().map(shape),
        reference.as_deref().map(shape),
        "structure changed"
    );
    if let Some(blocks) = &actual
        && !empty
    {
        let target = (context.target)(blocks);
        prop_assert!(target.is_some(), "value not found in {:?}", blocks);
        prop_assert_eq!(
            plain_text(target.unwrap_or_default()),
            (context.expected)(value)
        );
    }
    Ok(())
}

/// [`check`] for a context the escaping does not fully cover: a render error
/// is allowed, any document that does render must pass.
fn check_fail_closed(context: &Context, value: &str) -> Result<(), TestCaseError> {
    if (context.render)(value).is_none() {
        return Ok(());
    }
    check(context, value)
}

/// Input without `\n`, for the contexts a newline breaks (see the findings).
fn single_line(max_len: usize) -> impl Strategy<Value = String> {
    hostile_string(max_len).prop_map(|s| s.replace('\n', ""))
}

/// Input that does not start with whitespace once control characters drop,
/// for the emphasis contexts (see the findings).
fn no_leading_space(max_len: usize) -> impl Strategy<Value = String> {
    hostile_string(max_len).prop_map(|s| {
        s.trim_start_matches(|c: char| c.is_whitespace() || c.is_control())
            .to_string()
    })
}

proptest! {
    #![proptest_config(crate::test_support::strategies::parsed_config(1000))]

    #[test]
    fn paragraph_keeps_structure(v in hostile_string(40)) {
        check(&PARAGRAPH, &v)?;
    }

    #[test]
    fn inline_keeps_structure(v in hostile_string(40)) {
        check(&INLINE, &v)?;
    }

    #[test]
    fn bullet_item_keeps_structure(v in hostile_string(40)) {
        check(&BULLET, &v)?;
    }

    #[test]
    fn lettered_item_keeps_structure(v in hostile_string(40)) {
        check(&LETTERED, &v)?;
    }

    #[test]
    fn line_heading_keeps_structure(v in hostile_string(40)) {
        check(&HEADING, &v)?;
    }

    #[test]
    fn cell_keeps_structure(v in hostile_string(40)) {
        check(&CELL, &v)?;
    }

    #[test]
    fn mono_cell_keeps_structure(v in hostile_string(40)) {
        check(&MONO_CELL, &v)?;
    }

    /// Narrowed: a newline ends the quote (`quote_with_newline_keeps_structure`).
    #[test]
    fn quote_keeps_structure(v in single_line(40)) {
        check(&QUOTE, &v)?;
    }

    /// Narrowed: a newline ends the heading (`flow_heading_with_newline_keeps_structure`).
    #[test]
    fn flow_heading_keeps_structure(v in single_line(40)) {
        check(&FLOW_HEADING, &v)?;
    }

    /// Narrowed: leading whitespace detaches the opening `**`
    /// (`bold_with_leading_space_keeps_structure`).
    #[test]
    fn bold_keeps_structure(v in no_leading_space(40)) {
        check(&BOLD, &v)?;
    }

    /// Narrowed: both of the above (`bold_cell_with_newline_keeps_structure`).
    #[test]
    fn bold_cell_keeps_structure(v in no_leading_space(40).prop_map(|s| s.replace('\n', ""))) {
        check(&BOLD_CELL, &v)?;
    }

    /// Where escaping falls short, rendering fails instead of changing the
    /// structure.
    #[test]
    fn uncovered_contexts_fail_closed(v in hostile_string(40)) {
        for context in [&QUOTE, &FLOW_HEADING, &BOLD, &BOLD_CELL] {
            check_fail_closed(context, &v)?;
        }
    }

    /// Validated domain values, as the templates put them in these contexts,
    /// never hit the findings below.
    #[test]
    fn domain_values_in_uncovered_contexts(
        v in prop_oneof![
            appellation().prop_map(|a| a.to_string()),
            last_name().prop_map(|n| n.to_string()),
            initials().prop_map(|i| i.to_string()),
            locality().prop_map(|l| l.to_string()),
        ]
    ) {
        for context in [&QUOTE, &FLOW_HEADING, &BOLD, &BOLD_CELL] {
            check(context, &v)?;
        }
    }

    /// Long dangerous inputs, all contexts.
    #[test]
    fn long_dangerous_values(v in dangerous_string(200)) {
        for context in [&PARAGRAPH, &INLINE, &BULLET, &LETTERED, &HEADING, &CELL, &MONO_CELL] {
            check(context, &v)?;
        }
        for context in [&QUOTE, &FLOW_HEADING, &BOLD, &BOLD_CELL] {
            check_fail_closed(context, &v)?;
        }
    }
}

/// Runs a [`check`] outside proptest.
fn assert_check(context: &Context, value: &str) {
    if let Err(err) = check(context, value) {
        panic!("{value:?}: {err}");
    }
}

#[test]
#[ignore = "finding: a newline in a flow-escaped quote ends the quote (render error)"]
fn quote_with_newline_keeps_structure() {
    assert_check(&QUOTE, "a\nb");
}

#[test]
#[ignore = "finding: a newline in a flow-escaped heading ends the heading (render error)"]
fn flow_heading_with_newline_keeps_structure() {
    assert_check(&FLOW_HEADING, "a\nb");
}

#[test]
#[ignore = "finding: leading whitespace detaches the opening `**` (render error)"]
fn bold_with_leading_space_keeps_structure() {
    assert_check(&BOLD, " a");
}

#[test]
#[ignore = "finding: a newline in a flow-escaped table cell splits the row (render error)"]
fn bold_cell_with_newline_keeps_structure() {
    assert_check(&BOLD_CELL, "a\nb");
}
