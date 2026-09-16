use serde::Deserialize;

use crate::ElectionConfig;

/// Separate fields per election domain type so the form deserializes cleanly even when
/// JavaScript is disabled and every election domain picker submits a value.
#[derive(Deserialize)]
pub struct SwitchElectionForm {
    election: String,
    domain_province: Option<String>,
    domain_water_council: Option<String>,
}

impl SwitchElectionForm {
    pub fn into_election_config(self) -> Option<ElectionConfig> {
        // Try each submitted election domain in turn; only the one whose code matches
        // the election's domain type produces a valid config. Falls back to
        // `None` for domain-less elections.
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Province, WaterCouncil};

    fn parse(body: &str) -> SwitchElectionForm {
        serde_urlencoded::from_str(body).expect("form body")
    }

    #[test]
    fn deserializes_with_optional_domain_fields_absent() {
        let form = parse("election=EK27");
        assert_eq!(form.into_election_config(), Some(ElectionConfig::EK27));
    }

    #[test]
    fn ps27_uses_domain_province() {
        let form = parse("election=PS27&domain_province=prov1");
        assert_eq!(
            form.into_election_config(),
            Some(ElectionConfig::PS27(Province::Groningen))
        );
    }

    #[test]
    fn ws27_uses_domain_water_council() {
        let form = parse("election=WS27&domain_water_council=ws2");
        assert_eq!(
            form.into_election_config(),
            Some(ElectionConfig::WS27(WaterCouncil::Fryslan))
        );
    }

    #[test]
    fn ek27_ignores_submitted_domain_fields() {
        // When JS is disabled, every election domain picker submits a value.
        // The form should still resolve to EK27 because EK27 has no domain.
        let form = parse("election=EK27&domain_province=prov1&domain_water_council=ws2");
        assert_eq!(form.into_election_config(), Some(ElectionConfig::EK27));
    }

    #[test]
    fn ps27_ignores_unrelated_water_council_field() {
        // The province field is empty (placeholder option) but the water
        // council field is filled — it must not satisfy a PS27 election.
        let form = parse("election=PS27&domain_province=&domain_water_council=ws2");
        assert_eq!(form.into_election_config(), None);
    }

    #[test]
    fn ps27_with_empty_domain_returns_none() {
        let form = parse("election=PS27&domain_province=");
        assert_eq!(form.into_election_config(), None);
    }

    #[test]
    fn ps27_with_invalid_domain_returns_none() {
        let form = parse("election=PS27&domain_province=XX");
        assert_eq!(form.into_election_config(), None);
    }

    #[test]
    fn ps27_without_domain_returns_none() {
        let form = parse("election=PS27");
        assert_eq!(form.into_election_config(), None);
    }

    #[test]
    fn unknown_election_code_returns_none() {
        let form = parse("election=ZZ99&domain_province=prov1");
        assert_eq!(form.into_election_config(), None);
    }
}
