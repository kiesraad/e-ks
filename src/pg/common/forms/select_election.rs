use serde::Deserialize;

use crate::ElectionConfig;

/// Form for the post-login election selection page. Same domain semantics as
/// `SwitchElectionForm` but adds an optional `load_fixtures` toggle that only
/// has an effect when the `fixtures` feature is compiled in, and a
/// `login_as_csb` toggle that additionally needs `dev-features`.
#[derive(Deserialize)]
pub struct SelectElectionForm {
    election: String,
    domain_province: Option<String>,
    domain_water_council: Option<String>,
    load_fixtures: Option<String>,
    #[cfg(all(feature = "fixtures", feature = "dev-features"))]
    login_as_csb: Option<String>,
}

impl SelectElectionForm {
    pub fn election_config(&self) -> Option<ElectionConfig> {
        [
            self.domain_province.as_deref(),
            self.domain_water_council.as_deref(),
        ]
        .into_iter()
        .flatten()
        .filter(|s| !s.is_empty())
        .find_map(|d| ElectionConfig::from_code_and_domain(&self.election, Some(d)))
        .or_else(|| ElectionConfig::from_code_and_domain(&self.election, None))
    }

    pub fn load_fixtures(&self) -> bool {
        self.load_fixtures.is_some()
    }

    #[cfg(all(feature = "fixtures", feature = "dev-features"))]
    pub fn login_as_csb(&self) -> bool {
        self.login_as_csb.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Province;

    fn parse(body: &str) -> SelectElectionForm {
        serde_urlencoded::from_str(body).expect("form body")
    }

    #[test]
    fn deserializes_without_load_fixtures() {
        let form = parse("election=EK27");
        assert_eq!(form.election_config(), Some(ElectionConfig::EK27));
        assert!(!form.load_fixtures());
    }

    #[test]
    fn load_fixtures_checkbox_present_is_true() {
        let form = parse("election=EK27&load_fixtures=true");
        assert!(form.load_fixtures());
    }

    #[test]
    fn ps27_uses_domain_province() {
        let form = parse("election=PS27&domain_province=prov1");
        assert_eq!(
            form.election_config(),
            Some(ElectionConfig::PS27(Province::Groningen))
        );
    }
}
