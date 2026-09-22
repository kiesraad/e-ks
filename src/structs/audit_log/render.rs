//! Turning typed changes into the rows the audit-log templates render.
//!
//! This is the only place where a change meets a locale: labels are
//! translated, values are formatted, and entity references get their link.

use super::{AuditValue, Change, ChangeKind, EntityId, FieldKey, Move};
use crate::{Locale, constants::DEFAULT_DATE_FORMAT, trans};

/// What the render step needs besides the changes themselves.
pub struct RenderContext<'a> {
    pub locale: Locale,
    /// Where a reference to an entity links to; `None` renders it unlinked.
    pub link: Option<&'a dyn Fn(&EntityId) -> String>,
}

impl RenderContext<'_> {
    /// Render without entity links.
    pub fn without_links(locale: Locale) -> Self {
        RenderContext { locale, link: None }
    }
}

/// The rows of one group of fields, e.g. everything under "Gemachtigde".
#[derive(Debug, Clone, PartialEq)]
pub struct ChangeGroup {
    /// The group heading; `None` for fields at the root of the entity.
    pub label: Option<String>,
    pub rows: Vec<ChangeRow>,
}

/// One table row.
#[derive(Debug, Clone, PartialEq)]
pub struct ChangeRow {
    pub label: String,
    pub kind: RowKind,
    pub detail: RowDetail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    Added,
    Removed,
    Changed,
    Collection,
}

impl RowKind {
    pub fn css_class(&self) -> &'static str {
        match self {
            RowKind::Added => "added",
            RowKind::Removed => "removed",
            RowKind::Changed => "changed",
            RowKind::Collection => "collection",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum RowDetail {
    /// Old and new value side by side; either side may be empty.
    Scalar { old: Vec<Cell>, new: Vec<Cell> },
    /// A collection: what left, what came in, what changed place.
    Collection {
        removed: Vec<Cell>,
        added: Vec<Cell>,
        moved: Vec<MoveCell>,
    },
}

/// One rendered value.
#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    /// A prefix such as a list position (`#3`).
    pub marker: Option<String>,
    pub text: String,
    /// Present when the value is a reference to another entity.
    pub entity: Option<EntityLink>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EntityLink {
    pub id_full: String,
    pub href: Option<String>,
}

/// An item that changed place; positions are 1-based.
#[derive(Debug, Clone, PartialEq)]
pub struct MoveCell {
    pub cell: Cell,
    pub from: usize,
    pub to: usize,
}

/// Render `changes` as rows, grouped by the field group they belong to.
/// Consecutive changes in the same group share one [`ChangeGroup`].
pub fn render_groups(changes: &[Change], ctx: &RenderContext) -> Vec<ChangeGroup> {
    let mut groups: Vec<(&[FieldKey], ChangeGroup)> = Vec::new();

    for change in changes {
        let group = change.path.group();
        let row = render_row(change, ctx);
        match groups.last_mut() {
            Some((current, current_group)) if *current == group => current_group.rows.push(row),
            _ => groups.push((
                group,
                ChangeGroup {
                    label: change.path.group_label(ctx.locale),
                    rows: vec![row],
                },
            )),
        }
    }

    groups.into_iter().map(|(_, group)| group).collect()
}

fn render_row(change: &Change, ctx: &RenderContext) -> ChangeRow {
    let label = change.path.leaf_label(ctx.locale);
    let (kind, detail) = match &change.kind {
        ChangeKind::Added(value) => (
            RowKind::Added,
            RowDetail::Scalar {
                old: Vec::new(),
                new: cells(value, ctx),
            },
        ),
        ChangeKind::Removed(value) => (
            RowKind::Removed,
            RowDetail::Scalar {
                old: cells(value, ctx),
                new: Vec::new(),
            },
        ),
        ChangeKind::Changed { old, new } => (
            RowKind::Changed,
            RowDetail::Scalar {
                old: cells(old, ctx),
                new: cells(new, ctx),
            },
        ),
        ChangeKind::Collection {
            added,
            removed,
            moved,
        } => (
            RowKind::Collection,
            RowDetail::Collection {
                removed: removed.iter().flat_map(|value| cells(value, ctx)).collect(),
                added: added.iter().flat_map(|value| cells(value, ctx)).collect(),
                moved: moved.iter().map(|m| move_cell(m, ctx)).collect(),
            },
        ),
    };
    ChangeRow {
        label,
        kind,
        detail,
    }
}

fn move_cell(m: &Move, ctx: &RenderContext) -> MoveCell {
    MoveCell {
        cell: cells(&m.item, ctx)
            .into_iter()
            .next()
            .unwrap_or_else(|| Cell {
                marker: None,
                text: String::new(),
                entity: None,
            }),
        from: m.from,
        to: m.to,
    }
}

/// The cells a value renders as: none for a missing value, one per item for a
/// collection, one otherwise.
fn cells(value: &AuditValue, ctx: &RenderContext) -> Vec<Cell> {
    let single = |text: String| {
        vec![Cell {
            marker: None,
            text,
            entity: None,
        }]
    };
    match value {
        AuditValue::Missing => Vec::new(),
        AuditValue::Text(text) => single(text.clone()),
        AuditValue::Bool(true) => single(trans!("audit_log.detail.values.bool_true", ctx.locale)),
        AuditValue::Bool(false) => single(trans!("audit_log.detail.values.bool_false", ctx.locale)),
        AuditValue::Date(date) => single(date.format(DEFAULT_DATE_FORMAT).to_string()),
        AuditValue::Enum(value) => single(value.label(ctx.locale)),
        AuditValue::Entity(entity) => vec![Cell {
            marker: None,
            text: entity.description.clone(),
            entity: Some(EntityLink {
                id_full: entity.id.to_string(),
                href: ctx.link.map(|link| link(&entity.id)),
            }),
        }],
        AuditValue::Set(items) => items.iter().flat_map(|item| cells(item, ctx)).collect(),
        AuditValue::Ordered(items) => items
            .iter()
            .enumerate()
            .flat_map(|(index, item)| {
                cells(item, ctx).into_iter().map(move |mut cell| {
                    cell.marker = Some(format!("#{}", index + 1));
                    cell
                })
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::structs::{
        audit_log::{EntityRef, EnumValue, FieldPath},
        common::Gender,
        persons::PersonId,
    };

    fn text(value: &str) -> AuditValue {
        AuditValue::Text(value.to_string())
    }

    fn plain(text: &str) -> Cell {
        Cell {
            marker: None,
            text: text.to_string(),
            entity: None,
        }
    }

    #[test]
    fn formats_values_per_kind() {
        let ctx = RenderContext::without_links(Locale::Nl);

        assert_eq!(cells(&AuditValue::Bool(true), &ctx), vec![plain("Ja")]);
        assert_eq!(cells(&AuditValue::Bool(false), &ctx), vec![plain("Nee")]);
        assert_eq!(
            cells(
                &AuditValue::Date(chrono::NaiveDate::from_ymd_opt(1990, 2, 1).unwrap()),
                &ctx
            ),
            vec![plain("01-02-1990")]
        );
        assert_eq!(
            cells(&AuditValue::Enum(EnumValue::Gender(Gender::Female)), &ctx),
            vec![plain("vrouw")]
        );
        assert_eq!(cells(&AuditValue::Missing, &ctx), Vec::<Cell>::new());
    }

    #[test]
    fn ordered_items_get_position_markers() {
        let ctx = RenderContext::without_links(Locale::En);
        let cells = cells(&AuditValue::Ordered(vec![text("a"), text("b")]), &ctx);

        assert_eq!(
            cells
                .iter()
                .map(|c| c.marker.as_deref())
                .collect::<Vec<_>>(),
            vec![Some("#1"), Some("#2")]
        );
    }

    #[test]
    fn entity_gets_a_link_when_the_context_provides_one() {
        let id = PersonId::new();
        let entity = AuditValue::Entity(EntityRef {
            id: EntityId::Person(id),
            description: "Jansen, H.".to_string(),
        });
        let link = |entity: &EntityId| format!("/audit-log?search={entity}");

        let linked = cells(
            &entity,
            &RenderContext {
                locale: Locale::En,
                link: Some(&link),
            },
        );
        assert_eq!(
            linked[0].entity,
            Some(EntityLink {
                id_full: id.to_string(),
                href: Some(format!("/audit-log?search={id}")),
            })
        );
        assert_eq!(linked[0].text, "Jansen, H.");

        let unlinked = cells(&entity, &RenderContext::without_links(Locale::En));
        assert_eq!(unlinked[0].entity.as_ref().unwrap().href, None);
    }

    #[test]
    fn consecutive_changes_in_one_group_share_a_heading() {
        let changes = vec![
            Change::added(FieldKey::LastName, text("Jansen")),
            Change::added(
                FieldPath::from(FieldKey::Representative).with(FieldKey::LastName),
                text("Bos"),
            ),
            Change::added(
                FieldPath::from(FieldKey::Representative).with(FieldKey::Initials),
                text("E."),
            ),
        ];

        let groups = render_groups(&changes, &RenderContext::without_links(Locale::Nl));

        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].label, None);
        assert_eq!(groups[0].rows[0].label, "Achternaam");
        assert_eq!(groups[0].rows[0].kind, RowKind::Added);
        assert_eq!(groups[1].label.as_deref(), Some("Gemachtigde"));
        assert_eq!(groups[1].rows.len(), 2);
    }

    #[test]
    fn collection_rows_carry_removed_added_and_moved_cells() {
        let change = Change {
            path: FieldPath::from(FieldKey::Candidates),
            kind: ChangeKind::Collection {
                added: vec![text("new")],
                removed: vec![text("old")],
                moved: vec![Move {
                    item: text("mover"),
                    from: 3,
                    to: 1,
                }],
            },
        };

        let groups = render_groups(&[change], &RenderContext::without_links(Locale::En));
        let row = &groups[0].rows[0];

        assert_eq!(row.kind, RowKind::Collection);
        assert_eq!(
            row.detail,
            RowDetail::Collection {
                removed: vec![plain("old")],
                added: vec![plain("new")],
                moved: vec![MoveCell {
                    cell: plain("mover"),
                    from: 3,
                    to: 1,
                }],
            }
        );
    }
}
