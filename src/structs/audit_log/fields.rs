//! Listing the audit fields of an entity.
//!
//! An entity implements [`AuditFields`] by walking its own struct and pushing
//! one `(path, value)` pair per leaf. The [`audit_fields!`] macro writes that
//! implementation from a list that names every field once, and destructures
//! the struct without `..`, so a field that is left out (or added later) is a
//! compile error until the audit log knows how to show it.

use super::{AuditValue, FieldPath};

/// One row an entity contributes to the audit log: where it sits and its value.
pub type Field = (FieldPath, AuditValue);

/// An entity that can list its audit rows.
pub trait AuditFields {
    /// Push every leaf of `self` onto `out`, prefixed with `path`.
    fn audit_fields(&self, path: &FieldPath, out: &mut Vec<Field>);
}

impl<T: AuditFields> AuditFields for Option<T> {
    fn audit_fields(&self, path: &FieldPath, out: &mut Vec<Field>) {
        if let Some(inner) = self {
            inner.audit_fields(path, out);
        }
    }
}

/// Implement [`AuditFields`] for a struct by listing every field once as one of:
///
/// - `skip`: not shown (ids, timestamps, derived flags);
/// - `leaf(Key)`: a value shown under `FieldKey::Key`;
/// - `flatten`: a nested struct whose fields sit at this level;
/// - `group(Key)`: a nested struct whose fields are grouped under `FieldKey::Key`.
///
/// ```ignore
/// audit_fields!(Person {
///     id: skip,
///     name: flatten,
///     address: group(Address),
///     updated_at: skip,
/// });
/// ```
macro_rules! audit_fields {
    ($ty:ident { $( $field:ident : $spec:ident $(( $key:ident ))? ),* $(,)? }) => {
        impl $crate::structs::audit_log::AuditFields for $ty {
            fn audit_fields(
                &self,
                path: &$crate::structs::audit_log::FieldPath,
                out: &mut Vec<$crate::structs::audit_log::Field>,
            ) {
                let $ty { $( $field ),* } = self;
                $( $crate::structs::audit_log::audit_fields!(@field path, out, $field, $spec $(( $key ))?); )*
            }
        }
    };
    (@field $path:ident, $out:ident, $field:ident, skip) => {
        let _ = $field;
    };
    (@field $path:ident, $out:ident, $field:ident, flatten) => {
        $crate::structs::audit_log::AuditFields::audit_fields($field, $path, $out);
    };
    (@field $path:ident, $out:ident, $field:ident, group($key:ident)) => {
        $crate::structs::audit_log::AuditFields::audit_fields(
            $field,
            &$path.with($crate::structs::audit_log::FieldKey::$key),
            $out,
        );
    };
    (@field $path:ident, $out:ident, $field:ident, leaf($key:ident)) => {
        $out.push((
            $path.with($crate::structs::audit_log::FieldKey::$key),
            $crate::structs::audit_log::AuditLeaf::audit_value($field),
        ));
    };
}
pub(crate) use audit_fields;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::structs::audit_log::FieldKey;

    struct Inner {
        flag: Option<bool>,
    }
    audit_fields!(Inner {
        flag: leaf(KnownInBag)
    });

    struct Outer {
        id: u8,
        name: String,
        flat: Inner,
        grouped: Inner,
        optional: Option<Inner>,
    }
    audit_fields!(Outer {
        id: skip,
        name: leaf(LastName),
        flat: flatten,
        grouped: group(Address),
        optional: group(Representative),
    });

    #[test]
    fn lists_leaves_in_declaration_order_with_group_prefixes() {
        let outer = Outer {
            id: 7,
            name: "Jansen".to_string(),
            flat: Inner { flag: Some(true) },
            grouped: Inner { flag: None },
            optional: None,
        };
        let mut out = Vec::new();
        outer.audit_fields(&FieldPath::root(), &mut out);

        assert_eq!(
            out,
            vec![
                (
                    FieldPath::from(FieldKey::LastName),
                    AuditValue::Text("Jansen".to_string())
                ),
                (
                    FieldPath::from(FieldKey::KnownInBag),
                    AuditValue::Bool(true)
                ),
                (
                    FieldPath::from(FieldKey::Address).with(FieldKey::KnownInBag),
                    AuditValue::Missing
                ),
            ]
        );
    }

    #[test]
    fn present_option_is_listed_under_its_group() {
        let outer = Outer {
            id: 7,
            name: String::new(),
            flat: Inner { flag: None },
            grouped: Inner { flag: None },
            optional: Some(Inner { flag: Some(false) }),
        };
        let mut out = Vec::new();
        outer.audit_fields(&FieldPath::root(), &mut out);

        assert_eq!(
            out.last(),
            Some(&(
                FieldPath::from(FieldKey::Representative).with(FieldKey::KnownInBag),
                AuditValue::Bool(false)
            ))
        );
    }
}
