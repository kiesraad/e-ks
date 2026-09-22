//! Transparent string newtype macro.

/// This macro defines a newtype wrapper around a string new type
/// that derives common traits and is transparent for serialization and display purposes.
#[macro_export]
macro_rules! transparent_string {
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident(String);
    ) => {
        $(#[$meta])*
        #[derive(
            Default,
            Debug,
            Clone,
            PartialEq,
            Eq,
            PartialOrd,
            Ord,
            Hash,
            serde::Serialize,
            serde::Deserialize,
        )]
        #[serde(transparent)]
        $vis struct $name(String);

        impl std::ops::Deref for $name {
            type Target = String;

            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(f)
            }
        }

        impl $crate::structs::audit_log::AuditLeaf for $name {
            fn audit_value(&self) -> $crate::structs::audit_log::AuditValue {
                $crate::structs::audit_log::AuditLeaf::audit_value(&self.0)
            }
        }
    };
}
