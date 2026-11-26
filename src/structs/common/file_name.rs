const MAX_FILE_NAME_LEN: usize = 255;

crate::transparent_string! {
    /// The name of a file a user uploaded, as the browser reported it. It is
    /// only ever shown and logged, never used as a path.
    pub struct FileName(String);
}

impl FileName {
    /// Strip path components and control characters from an uploaded file
    /// name, and truncate to a bounded length. Returns `None` if nothing is
    /// left.
    pub fn from_upload(raw: &str) -> Option<Self> {
        let basename = raw.rsplit(['/', '\\']).next().unwrap_or_default();

        let cleaned: String = basename
            .chars()
            .filter(|c| !c.is_control())
            .take(MAX_FILE_NAME_LEN)
            .collect();

        let trimmed = cleaned.trim();
        (!trimmed.is_empty()).then(|| Self(trimmed.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sanitized(raw: &str) -> Option<String> {
        FileName::from_upload(raw).map(|name| name.to_string())
    }

    #[test]
    fn strips_paths_and_control_chars() {
        assert_eq!(sanitized("../../etc/passwd").as_deref(), Some("passwd"));
        assert_eq!(
            sanitized(r"C:\Users\x\report.csv").as_deref(),
            Some("report.csv")
        );
        assert_eq!(
            sanitized("bad\x00name\n.csv").as_deref(),
            Some("badname.csv")
        );
        assert_eq!(sanitized("   "), None);
        assert_eq!(sanitized(""), None);
    }

    #[test]
    fn truncates_to_max_length() {
        let long = "a".repeat(500) + ".csv";
        let name = FileName::from_upload(&long).unwrap();
        assert_eq!(name.chars().count(), MAX_FILE_NAME_LEN);
    }
}
