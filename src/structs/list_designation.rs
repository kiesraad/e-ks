//! How a political group presents its list (Kieswet Art. G 1, H 3) and which
//! committee registrations it is bound to: a standalone list carries one
//! registered appellation, a combined list several, a blank list none.

use crate::{
    form::ValidationError,
    structs::{
        audit_log::{AuditFields, AuditLeaf, AuditValue, Field, FieldKey, FieldPath, Subject},
        common::{InfoProblems, Problematic, Problems},
        csb::RegisteredPoliticalGroupId,
    },
};
use serde::{Deserialize, Serialize};

/// The three list types, as the list-type form, the H-models and the audit
/// log name them. Also the bare form the store held before registrations were
/// bound.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ListDesignationKind {
    #[default]
    Standalone,
    Blank,
    Combined,
}

/// The list type with the registrations it is bound to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", from = "ListDesignationRepr")]
pub enum ListDesignation {
    /// A registered appellation; `None` while the group enters its
    /// information by hand.
    Standalone {
        #[serde(default)]
        registration: Option<RegisteredPoliticalGroupId>,
    },
    /// A combination of registered appellations.
    Combined {
        #[serde(default)]
        registrations: Vec<RegisteredPoliticalGroupId>,
    },
    Blank,
}

/// Stored before registrations were bound: the bare kind. Both forms are read.
#[derive(Deserialize)]
#[serde(untagged)]
enum ListDesignationRepr {
    Tagged(ListDesignationTagged),
    Bare(ListDesignationKind),
}

/// The wire form of [`ListDesignation`] itself, which `from` replaced.
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum ListDesignationTagged {
    Standalone {
        #[serde(default)]
        registration: Option<RegisteredPoliticalGroupId>,
    },
    Combined {
        #[serde(default)]
        registrations: Vec<RegisteredPoliticalGroupId>,
    },
    Blank,
}

impl From<ListDesignationRepr> for ListDesignation {
    fn from(repr: ListDesignationRepr) -> Self {
        match repr {
            ListDesignationRepr::Tagged(ListDesignationTagged::Standalone { registration }) => {
                ListDesignation::Standalone { registration }
            }
            ListDesignationRepr::Tagged(ListDesignationTagged::Combined { registrations }) => {
                ListDesignation::Combined { registrations }
            }
            ListDesignationRepr::Tagged(ListDesignationTagged::Blank) => ListDesignation::Blank,
            ListDesignationRepr::Bare(kind) => kind.into(),
        }
    }
}

impl Default for ListDesignation {
    fn default() -> Self {
        ListDesignationKind::default().into()
    }
}

/// The list type without any registration bound to it.
impl From<ListDesignationKind> for ListDesignation {
    fn from(kind: ListDesignationKind) -> Self {
        match kind {
            ListDesignationKind::Standalone => ListDesignation::Standalone { registration: None },
            ListDesignationKind::Combined => ListDesignation::Combined {
                registrations: Vec::new(),
            },
            ListDesignationKind::Blank => ListDesignation::Blank,
        }
    }
}

impl ListDesignation {
    pub fn kind(&self) -> ListDesignationKind {
        match self {
            ListDesignation::Standalone { .. } => ListDesignationKind::Standalone,
            ListDesignation::Combined { .. } => ListDesignationKind::Combined,
            ListDesignation::Blank => ListDesignationKind::Blank,
        }
    }

    pub fn is_blank(&self) -> bool {
        self.kind() == ListDesignationKind::Blank
    }

    pub fn is_combined(&self) -> bool {
        self.kind() == ListDesignationKind::Combined
    }

    /// The registrations the list is bound to.
    pub fn registrations(&self) -> &[RegisteredPoliticalGroupId] {
        match self {
            ListDesignation::Standalone {
                registration: Some(registration),
            } => std::slice::from_ref(registration),
            ListDesignation::Combined { registrations } => registrations,
            ListDesignation::Standalone { registration: None } | ListDesignation::Blank => &[],
        }
    }
}

/// The list type as one row and the registrations bound to it as another,
/// so a change of binding shows up in the audit log.
impl AuditFields for Option<ListDesignation> {
    fn audit_fields(&self, path: &FieldPath, out: &mut Vec<Field>) {
        out.push((
            path.with(FieldKey::ListDesignation),
            self.as_ref().map(ListDesignation::kind).audit_value(),
        ));
        let registrations = self
            .as_ref()
            .map(ListDesignation::registrations)
            .unwrap_or_default()
            .iter()
            .map(|id| AuditValue::entity(Subject::RegisteredPoliticalGroup(*id), None))
            .collect();
        out.push((
            path.with(FieldKey::RegisteredPoliticalGroup),
            AuditValue::Set(registrations),
        ));
    }
}

impl Problematic<()> for Option<ListDesignation> {
    fn get_problems(&self, _: ()) -> Problems {
        Problems {
            info_problems: if self.is_none() {
                vec![InfoProblems::NoListDesignation]
            } else {
                Vec::new()
            },
            potential_problems: Vec::new(),
        }
    }
}

impl std::str::FromStr for ListDesignationKind {
    type Err = ValidationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "standalone" => Ok(ListDesignationKind::Standalone),
            "blank" => Ok(ListDesignationKind::Blank),
            "combined" => Ok(ListDesignationKind::Combined),
            _ => Err(ValidationError::InvalidValue),
        }
    }
}

impl std::fmt::Display for ListDesignationKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ListDesignationKind::Standalone => write!(f, "standalone"),
            ListDesignationKind::Blank => write!(f, "blank"),
            ListDesignationKind::Combined => write!(f, "combined"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        store::encoding::{decode, encode},
        structs::{common::PreviousElectionResults, political_groups::PoliticalGroup},
    };
    use serde_json::json;

    #[test]
    fn kinds_round_trip_through_unbound_designations() {
        for kind in [
            ListDesignationKind::Standalone,
            ListDesignationKind::Blank,
            ListDesignationKind::Combined,
        ] {
            let designation = ListDesignation::from(kind);
            assert_eq!(designation.kind(), kind);
            assert!(designation.registrations().is_empty());
            assert_eq!(kind.to_string().parse::<ListDesignationKind>(), Ok(kind));
        }
        assert_eq!(
            ListDesignation::default().kind(),
            ListDesignationKind::Standalone
        );
        assert!(ListDesignation::Blank.is_blank());
        assert!(ListDesignation::from(ListDesignationKind::Combined).is_combined());
    }

    #[test]
    fn bound_designations_list_their_registrations() {
        let a = RegisteredPoliticalGroupId::new();
        let b = RegisteredPoliticalGroupId::new();
        assert_eq!(
            ListDesignation::Standalone {
                registration: Some(a)
            }
            .registrations(),
            [a]
        );
        assert_eq!(
            ListDesignation::Combined {
                registrations: vec![a, b]
            }
            .registrations(),
            [a, b]
        );
    }

    #[test]
    fn audit_rows_list_the_kind_and_the_registrations() {
        let id = RegisteredPoliticalGroupId::new();
        let mut rows = Vec::new();
        Some(ListDesignation::Standalone {
            registration: Some(id),
        })
        .audit_fields(&FieldPath::root(), &mut rows);
        assert_eq!(
            rows,
            [
                (
                    FieldPath::root().with(FieldKey::ListDesignation),
                    ListDesignationKind::Standalone.audit_value()
                ),
                (
                    FieldPath::root().with(FieldKey::RegisteredPoliticalGroup),
                    AuditValue::Set(vec![AuditValue::entity(
                        Subject::RegisteredPoliticalGroup(id),
                        None
                    )])
                ),
            ]
        );

        let mut rows = Vec::new();
        None::<ListDesignation>.audit_fields(&FieldPath::root(), &mut rows);
        assert_eq!(rows[0].1, AuditValue::Missing);
        assert_eq!(rows[1].1, AuditValue::Set(Vec::new()));
    }

    #[test]
    fn bare_strings_still_decode() {
        for (text, kind) in [
            ("standalone", ListDesignationKind::Standalone),
            ("blank", ListDesignationKind::Blank),
            ("combined", ListDesignationKind::Combined),
        ] {
            let from_json: ListDesignation = serde_json::from_value(json!(text)).unwrap();
            assert_eq!(from_json, ListDesignation::from(kind));
            let from_cbor: ListDesignation = decode(&encode(&kind).unwrap()).unwrap();
            assert_eq!(from_cbor, ListDesignation::from(kind));
        }
    }

    #[test]
    fn tagged_form_round_trips() {
        let id = RegisteredPoliticalGroupId::new();
        let standalone = ListDesignation::Standalone {
            registration: Some(id),
        };
        assert_eq!(
            serde_json::to_value(&standalone).unwrap(),
            json!({ "type": "standalone", "registration": id.to_string() })
        );
        assert_eq!(
            serde_json::to_value(ListDesignation::Blank).unwrap(),
            json!({ "type": "blank" })
        );
        for designation in [
            standalone,
            ListDesignation::Blank,
            ListDesignation::Combined {
                registrations: vec![id, RegisteredPoliticalGroupId::new()],
            },
        ] {
            let json = serde_json::to_value(&designation).unwrap();
            assert_eq!(
                serde_json::from_value::<ListDesignation>(json).unwrap(),
                designation
            );
            let cbor = encode(&designation).unwrap();
            assert_eq!(decode::<ListDesignation>(&cbor).unwrap(), designation);
        }
    }

    /// A political group as stored before registrations were bound, the
    /// shape inside old `UpdatePoliticalGroup` events and import snapshots.
    #[derive(Serialize)]
    struct OldPoliticalGroup {
        appellation: Option<String>,
        list_designation: Option<ListDesignationKind>,
        previous_election_results: Option<PreviousElectionResults>,
    }

    #[test]
    fn an_old_political_group_snapshot_decodes() {
        for (kind, expected) in [
            (
                Some(ListDesignationKind::Combined),
                Some(ListDesignation::Combined {
                    registrations: Vec::new(),
                }),
            ),
            (None, None),
        ] {
            let old = OldPoliticalGroup {
                appellation: Some("Partij".to_string()),
                list_designation: kind,
                previous_election_results: Some(PreviousElectionResults::ZeroSeats),
            };
            let group: PoliticalGroup = decode(&encode(&old).unwrap()).unwrap();
            assert_eq!(group.list_designation, expected);
            assert_eq!(group.appellation.unwrap().to_string(), "Partij");
        }
    }
}
