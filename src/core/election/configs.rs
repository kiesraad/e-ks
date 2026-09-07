use chrono::{Datelike, NaiveDate, NaiveDateTime, NaiveTime};

use crate::{
    ElectoralDistrict,
    core::{
        AnyLocale, ElectionType, ModelLocale,
        election::{Province, PublicSession, WaterCouncil},
    },
};

const fn date(year: i32, month: u32, day: u32) -> NaiveDate {
    match NaiveDate::from_ymd_opt(year, month, day) {
        Some(date) => date,
        None => panic!("invalid constant date"),
    }
}

const fn at(date: NaiveDate, hour: u32, minute: u32) -> NaiveDateTime {
    match NaiveTime::from_hms_opt(hour, minute, 0) {
        Some(time) => NaiveDateTime::new(date, time),
        None => panic!("invalid constant time"),
    }
}

super::define_elections! {
    EK27 {
        election_type: ElectionType::Ek,
        titles: {
            nl: "Eerste Kamerverkiezing der Staten-Generaal 2027",
            fry: "Earste Keamerferkiezings fan de Steaten-Generaal 2027",
            en: "Election of the Senate of the States General 2027",
        },
        electoral_districts: ElectoralDistrict::ek_districts(),
        number_of_seats: 75,
        eligible_date_of_birth: const { date(2014, 4, 20) }, // TODO: determine definitive date
        nomination_day_date: const { date(2027, 4, 20) },
        // Estimated from EK 2023 planning (official 2027 planning not yet published)
        document_review_date: const { date(2027, 4, 25) },
        omission_period_end_date: const { date(2027, 4, 29) },
        public_session: PublicSession {
            location: "'s-Gravenhage",
            datetime: const { at(date(2027, 5, 3), 17, 0) },
            chair: "",
            members: &["", "", "", "", ""],
        },
        election_date: const { date(2027, 5, 24) }
    },

    PS27(province: Province) {
        election_type: ElectionType::Ps,
        titles: {
            nl: "Provinciale Statenverkiezingen 2027",
            fry: "Provinsjale Steateferkiezings 2027",
            en: "Elections of the Provincial Council 2027",
        },
        electoral_districts: province.ps_districts(),
        number_of_seats: match province {
            Province::Groningen => 43,
            Province::Fryslan => 43,
            Province::Drenthe => 43,
            Province::Overijssel => 47,
            Province::Flevoland => 41,
            Province::Gelderland => 55,
            Province::Utrecht => 49,
            Province::NoordHolland => 55,
            Province::ZuidHolland => 55,
            Province::Zeeland => 39,
            Province::NoordBrabant => 55,
            Province::Limburg => 47,
        },
        eligible_date_of_birth: const { date(2014, 2, 1) }, // TODO: determine definitive date
        nomination_day_date: const { date(2027, 2, 1) },
        document_review_date: const { date(2027, 2, 2) },
        omission_period_end_date: const { date(2027, 2, 4) },
        public_session: PublicSession {
            location: "'s-Gravenhage",
            datetime: const { at(date(2027, 2, 5), 17, 0) },
            chair: "",
            members: &["", "", "", "", ""],
        },
        election_date: const { date(2027, 3, 17) }
    },

    WS27(water_council: WaterCouncil) {
        election_type: ElectionType::Ws,
        titles: {
            nl: "Waterschapsverkiezingen 2027",
            fry: "Wetterskipsferkiezings 2027",
            en: "Elections of the Water Authority 2027",
        },
        electoral_districts: water_council.ws_districts(),
        number_of_seats: match water_council {
            WaterCouncil::AaEnMaas => 26,
            WaterCouncil::AmstelGooiEnVecht => 26,
            WaterCouncil::BrabantseDelta => 26,
            WaterCouncil::DeDommel => 26,
            WaterCouncil::DeStichtseRijnlanden => 26,
            WaterCouncil::Delfland => 26,
            WaterCouncil::DrentsOverijsselseDelta => 25,
            WaterCouncil::HollandseDelta => 26,
            WaterCouncil::HollandsNoorderkwartier => 26,
            WaterCouncil::HunzeEnAas => 19,
            WaterCouncil::Fryslan => 21,
            WaterCouncil::Limburg => 26,
            WaterCouncil::Noorderzijlvest => 19,
            WaterCouncil::RijnEnIJssel => 30,
            WaterCouncil::Rijnland => 26,
            WaterCouncil::Rivierenland => 26,
            WaterCouncil::SchielandEnDeKrimpenerwaard => 26,
            WaterCouncil::Scheldestromen => 26,
            WaterCouncil::ValleiEnVeluwe => 26,
            WaterCouncil::Vechtstromen => 23,
            WaterCouncil::Zuiderzeeland => 21,
        },
        eligible_date_of_birth: const { date(2014, 2, 1) }, // TODO: determine definitive date
        nomination_day_date: const { date(2027, 2, 1) },
        document_review_date: const { date(2027, 2, 2) },
        omission_period_end_date: const { date(2027, 2, 4) },
        public_session: PublicSession {
            location: "'s-Gravenhage",
            datetime: const { at(date(2027, 2, 5), 17, 0) },
            chair: "",
            members: &["", "", "", "", ""],
        },
        election_date: const { date(2027, 3, 17) }
    }
}

impl ElectionConfig {
    /// Stable ID for the election configuration, used in HKDF derivation.
    pub fn stable_id(&self) -> String {
        let code = self.code();

        if let Some(region_code) = self.region_code() {
            format!("{code}:{region_code}")
        } else {
            code.to_string()
        }
    }

    /// The election code as a download filename slug, e.g. `ek27`, `ps27prov1`.
    pub fn filename_slug(&self) -> String {
        let mut slug = self.code().to_lowercase();
        if let Some(region) = self.region_code() {
            slug.push_str(&region.to_lowercase());
        }
        slug
    }

    /// Parse a [`Self::stable_id`] string (e.g. `"EK27"`, `"PS27:prov1"`)
    /// back to an election configuration.
    pub fn from_stable_id(value: &str) -> Option<Self> {
        let (code, region) = match value.split_once(':') {
            Some((code, region)) => (code, Some(region)),
            None => (value, None),
        };
        Self::from_code_and_region(code, region)
    }

    /// The election title to be followed by the phrase "Het gaat om de verkiezing van ...", as written on the models.
    ///
    /// Specifies the region, but not the year of the election.
    pub fn formal_title(&self, locale: ModelLocale) -> String {
        let region = || {
            self.region_title()
                .expect("region title required for this election type")
        };

        match (self.election_type(), locale) {
            (ElectionType::Tk, ModelLocale::Nl) => {
                "de Tweede Kamer der Staten-Generaal".to_string()
            }
            (ElectionType::Tk, ModelLocale::Fry) => {
                "de Twadde Keamer fan de Steaten-Generaal".to_string()
            }

            (ElectionType::Ek, ModelLocale::Nl) => {
                "de Eerste Kamer der Staten-Generaal".to_string()
            }
            (ElectionType::Ek, ModelLocale::Fry) => {
                "de Earste Keamer fan de Steaten-Generaal".to_string()
            }

            (ElectionType::Gr, ModelLocale::Nl) => {
                format!("de gemeenteraad van {}", region())
            }
            (ElectionType::Gr, ModelLocale::Fry) => {
                format!("de gemeenterie fan {}", region())
            }

            (ElectionType::Ps, ModelLocale::Nl) => {
                format!("de provinciale staten van {}", region())
            }
            (ElectionType::Ps, ModelLocale::Fry) => {
                format!("de Provinsjale Steaten fan {}", region())
            }

            (ElectionType::Ws, ModelLocale::Nl) => {
                format!("het algemeen bestuur van het waterschap {}", region())
            }
            (ElectionType::Ws, ModelLocale::Fry) => {
                format!("it algemien bestjoer fan it wetterskip {}", region())
            }

            (ElectionType::Ep, ModelLocale::Nl) => "het Europees Parlement".to_string(),
            (ElectionType::Ep, ModelLocale::Fry) => "het Europees Parlement".to_string(),

            (ElectionType::Kc, _) => todo!("Support electoral college regions"),
            (ElectionType::Kcni, _) => todo!("Support non-resident electoral college regions"),
            (ElectionType::Er, _) => todo!("Support island regions"),
        }
    }

    /// The full formal election title including the region and year, as listed in the EML 210.
    ///
    /// E.g. "Verkiezing van de gemeenteraad van Voorne aan Zee 2026"
    pub fn full_formal_title(&self, locale: ModelLocale) -> String {
        format!(
            "{} {} {}",
            match locale {
                ModelLocale::Fry => "Ferkiezing fan",
                ModelLocale::Nl => "Verkiezing van",
            },
            self.formal_title(locale),
            self.election_date().year()
        )
    }

    /// Returns all concrete election configurations.
    pub fn all() -> Vec<ElectionConfig> {
        let mut configs = vec![ElectionConfig::EK27];
        configs.extend(Province::ALL.iter().map(|p| ElectionConfig::PS27(*p)));
        configs.extend(WaterCouncil::ALL.iter().map(|wc| ElectionConfig::WS27(*wc)));
        configs
    }

    /// Returns one representative `ElectionConfig` per election type, for the
    /// type-selector dropdown. Derived from `ElectionConfig::all()` so new
    /// election types are picked up automatically.
    pub fn type_options() -> Vec<ElectionConfig> {
        let mut seen = std::collections::HashSet::new();
        Self::all()
            .into_iter()
            .filter(|e| seen.insert(e.code()))
            .collect()
    }

    pub fn available_districts(
        &self,
        used_districts: Vec<ElectoralDistrict>,
    ) -> Vec<ElectoralDistrict> {
        self.electoral_districts()
            .iter()
            .filter(|d| !used_districts.contains(d))
            .cloned()
            .collect()
    }

    pub fn has_only_one_district(&self) -> bool {
        self.electoral_districts().len() == 1
    }

    pub fn nineteen_or_more_seats(&self) -> bool {
        self.number_of_seats() >= 19
    }
}

#[cfg(test)]
mod tests {
    use crate::Locale;

    use super::*;

    #[test]
    fn election_titles_are_correct() {
        assert!(ElectionConfig::EK27.title(AnyLocale::Nl).len() > 20);

        let election_type = ElectionConfig::EK27.election_type();
        assert!(election_type.title(Locale::Nl).len() > 20);
    }

    #[test]
    fn election_config_exposes_districts() {
        let districts = ElectionConfig::EK27.electoral_districts();
        assert!(districts.contains(&ElectoralDistrict::NoordHolland));

        let districts = ElectionConfig::PS27(Province::Gelderland).electoral_districts();
        assert!(districts.contains(&ElectoralDistrict::PsNijmegen));

        let districts = ElectionConfig::WS27(WaterCouncil::AaEnMaas).electoral_districts();
        assert_eq!(districts, &[ElectoralDistrict::WsAaEnMaas]);
        let districts = ElectionConfig::WS27(WaterCouncil::Rivierenland).electoral_districts();
        assert_eq!(districts, &[ElectoralDistrict::WsRivierenland]);
        let districts = ElectionConfig::WS27(WaterCouncil::ValleiEnVeluwe).electoral_districts();
        assert_eq!(districts, &[ElectoralDistrict::WsValleiEnVeluwe]);
    }

    #[test]
    fn has_only_district() {
        assert!(ElectionConfig::PS27(Province::Drenthe).has_only_one_district());
        assert!(!ElectionConfig::PS27(Province::Gelderland).has_only_one_district());
    }

    #[test]
    fn type_options_contains_one_per_election_code() {
        let options = ElectionConfig::type_options();

        let codes: Vec<&str> = options.iter().map(ElectionConfig::code).collect();
        assert_eq!(codes, vec!["EK27", "PS27", "WS27"]);

        // No duplicate codes — each election type appears at most once.
        let mut sorted = codes.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), codes.len());
    }

    #[test]
    fn from_code_and_region_resolves_region_less_election() {
        assert_eq!(
            ElectionConfig::from_code_and_region("EK27", None),
            Some(ElectionConfig::EK27)
        );
    }

    #[test]
    fn from_code_and_region_ignores_region_for_region_less_election() {
        // A spurious region argument is ignored for elections that don't take one.
        assert_eq!(
            ElectionConfig::from_code_and_region("EK27", Some("anything")),
            Some(ElectionConfig::EK27)
        );
    }

    #[test]
    fn from_code_and_region_resolves_ps27_with_valid_province() {
        assert_eq!(
            ElectionConfig::from_code_and_region("PS27", Some("prov1")),
            Some(ElectionConfig::PS27(Province::Groningen))
        );
    }

    #[test]
    fn from_code_and_region_resolves_ws27_with_valid_water_council() {
        assert_eq!(
            ElectionConfig::from_code_and_region("WS27", Some("ws2")),
            Some(ElectionConfig::WS27(WaterCouncil::Fryslan))
        );
    }

    #[test]
    fn from_code_and_region_returns_none_when_region_required_but_missing() {
        assert_eq!(ElectionConfig::from_code_and_region("PS27", None), None);
        assert_eq!(ElectionConfig::from_code_and_region("WS27", None), None);
    }

    #[test]
    fn from_code_and_region_returns_none_for_invalid_region() {
        assert_eq!(
            ElectionConfig::from_code_and_region("PS27", Some("XX")),
            None
        );
        assert_eq!(
            ElectionConfig::from_code_and_region("WS27", Some("NotAWaterCouncil")),
            None
        );
    }

    #[test]
    fn from_code_and_region_returns_none_for_unknown_code() {
        assert_eq!(
            ElectionConfig::from_code_and_region("ZZ99", Some("GR")),
            None
        );
        assert_eq!(ElectionConfig::from_code_and_region("", None), None);
    }

    #[test]
    fn formal_title() {
        assert_eq!(
            ElectionConfig::EK27.formal_title(ModelLocale::Nl),
            "de Eerste Kamer der Staten-Generaal"
        );
        assert_eq!(
            ElectionConfig::EK27.formal_title(ModelLocale::Fry),
            "de Earste Keamer fan de Steaten-Generaal"
        );
        assert_eq!(
            ElectionConfig::EK27.full_formal_title(ModelLocale::Nl),
            "Verkiezing van de Eerste Kamer der Staten-Generaal 2027"
        );
        assert_eq!(
            ElectionConfig::EK27.full_formal_title(ModelLocale::Fry),
            "Ferkiezing fan de Earste Keamer fan de Steaten-Generaal 2027"
        );

        assert_eq!(
            ElectionConfig::PS27(Province::Drenthe).formal_title(ModelLocale::Nl),
            "de provinciale staten van Drenthe"
        );
        assert_eq!(
            ElectionConfig::PS27(Province::Drenthe).full_formal_title(ModelLocale::Nl),
            "Verkiezing van de provinciale staten van Drenthe 2027"
        );

        assert_eq!(
            ElectionConfig::WS27(WaterCouncil::Fryslan).formal_title(ModelLocale::Fry),
            "it algemien bestjoer fan it wetterskip Fryslân"
        );
        assert_eq!(
            ElectionConfig::WS27(WaterCouncil::Fryslan).full_formal_title(ModelLocale::Fry),
            "Ferkiezing fan it algemien bestjoer fan it wetterskip Fryslân 2027"
        );
    }
}
