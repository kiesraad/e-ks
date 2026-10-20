//! A description of a JSON decode failure that names the field, never the
//! value: the documents this application decodes hold personal data, and
//! serde's own messages quote the value they choked on.

use serde_json::error::Category;

/// Where and how `err` failed, without quoting the input. `err` comes from
/// `serde_path_to_error::deserialize`, which tracks the field path.
pub fn describe_json_error(err: &serde_path_to_error::Error<serde_json::Error>) -> String {
    let what = match err.inner().classify() {
        Category::Io => "could not be read",
        Category::Syntax => "is not valid JSON",
        Category::Eof => "ends early",
        Category::Data => "does not fit the expected shape",
    };
    let path = err.path().to_string();
    // The root, which serde_path_to_error writes as "."
    if path == "." {
        format!("document {what}")
    } else {
        format!("value at `{path}` {what}")
    }
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::*;

    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct Person {
        name: u32,
    }

    #[test]
    fn names_the_field_but_not_the_value() {
        let json = serde_json::json!({"person": {"name": "Jansen"}});

        #[derive(Debug, Deserialize)]
        #[allow(dead_code)]
        struct Document {
            person: Person,
        }
        let err = serde_path_to_error::deserialize::<_, Document>(json).unwrap_err();
        let description = describe_json_error(&err);

        assert!(description.contains("`person.name`"), "{description}");
        assert!(!description.contains("Jansen"), "{description}");
    }

    /// The session identity is an externally tagged enum; the path runs
    /// through the variant to the field.
    #[test]
    fn the_path_runs_through_an_enum_variant() {
        #[derive(Debug, Deserialize)]
        #[allow(dead_code)]
        enum Identity {
            PoliticalGroup { saml_name_id: u32 },
        }
        let json = serde_json::json!({"PoliticalGroup": {"saml_name_id": "secret"}});
        let err = serde_path_to_error::deserialize::<_, Identity>(json).unwrap_err();
        let description = describe_json_error(&err);

        assert!(
            description.contains("`PoliticalGroup.saml_name_id`"),
            "{description}"
        );
        assert!(!description.contains("secret"), "{description}");
    }

    #[test]
    fn malformed_json_is_reported_as_such() {
        let mut json = serde_json::Deserializer::from_str(r#"{"name": Jansen"#);
        let err = serde_path_to_error::deserialize::<_, Person>(&mut json).unwrap_err();
        let description = describe_json_error(&err);

        assert!(description.contains("not valid JSON"), "{description}");
        assert!(!description.contains("Jansen"), "{description}");
    }
}
