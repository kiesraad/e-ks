use serde::{Deserialize, Serialize};

use crate::{
    ElectionConfig, OptionAsStrExt,
    structs::common::{
        BsnOrNoneConfirmed, CountryCode, DateOfBirth, Gender, InfoProblems, PlaceOfResidence,
        PotentialProblems, Problematic, Problems,
    },
};

#[derive(Default, Debug, Serialize, Deserialize, Eq, PartialEq, Clone)]
pub struct PersonalData {
    pub gender: Option<Gender>,

    pub bsn: Option<BsnOrNoneConfirmed>,
    pub date_of_birth: Option<DateOfBirth>,

    pub place_of_residence: Option<PlaceOfResidence>,
    pub country: Option<CountryCode>,
}

impl Problematic<ElectionConfig> for PersonalData {
    fn get_problems(&self, election: ElectionConfig) -> Problems {
        let mut potential_problems = Vec::new();
        let mut info_problems = Vec::new();

        if self.bsn.is_none() {
            potential_problems.push(PotentialProblems::NoBsn);
        }

        match &self.place_of_residence {
            None => potential_problems.push(PotentialProblems::NoPlaceOfResidence),
            Some(p) if p.as_str().is_empty() => {
                potential_problems.push(PotentialProblems::NoPlaceOfResidence)
            }
            Some(PlaceOfResidence::Unknown(_)) if self.lives_in_nl() => {
                potential_problems.push(PotentialProblems::UnknownPlaceOfResidence)
            }
            _ => {}
        }

        if self.country.is_empty_or_none() {
            potential_problems.push(PotentialProblems::NoCountryOfResidence);
        }

        match &self.date_of_birth {
            None => potential_problems.push(PotentialProblems::NoDateOfBirth),
            Some(dob) => {
                if dob.is_very_old() {
                    info_problems.push(InfoProblems::VeryOldDateOfBirth);
                }
                if dob.is_too_young(&election) {
                    potential_problems.push(PotentialProblems::TooYoungDateOfBirth);
                }
            }
        }

        Problems {
            potential_problems,
            info_problems,
        }
    }
}

impl PersonalData {
    pub fn lives_in_nl(&self) -> bool {
        self.country.as_ref().is_none_or(CountryCode::is_nl)
    }

    pub fn show_unknown_place_of_residence_warning(&self) -> bool {
        self.lives_in_nl() && PlaceOfResidence::is_unknown_opt(&self.place_of_residence)
    }

    /// Whether this person needs an authorised person (gemachtigde) instead of
    /// a Dutch correspondence address: living abroad, or in the Caribbean
    /// Netherlands (which has country code NL, but no Dutch postal addresses).
    pub fn needs_representative(&self) -> bool {
        !self.lives_in_nl()
            || self
                .place_of_residence
                .as_ref()
                .is_some_and(PlaceOfResidence::is_caribbean_nl)
    }

    pub fn locality(&self) -> Option<String> {
        match (&self.place_of_residence, &self.country) {
            (Some(place), Some(country)) if !country.is_nl() => {
                Some(format!("{} ({})", place, country))
            }
            (Some(place), _) => Some(place.to_string()),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PgStore;
    use chrono::{Duration, NaiveDate};

    fn complete_personal_data() -> PersonalData {
        PersonalData {
            gender: None,
            bsn: Some(BsnOrNoneConfirmed::NoneConfirmed),
            date_of_birth: Some("01-01-1990".parse().unwrap()),
            place_of_residence: Some("Amsterdam".parse().unwrap()),
            country: Some("NL".parse().unwrap()),
        }
    }

    #[test]
    fn complete_personal_data_has_no_problems() {
        let problems = complete_personal_data().get_problems(ElectionConfig::EK27);
        assert!(problems.potential_problems.is_empty());
        assert!(problems.info_problems.is_empty());
    }

    #[test]
    fn needs_representative_when_living_abroad_or_in_caribbean_nl() {
        let mut data = complete_personal_data();
        assert!(!data.needs_representative());

        data.country = Some("BE".parse().unwrap());
        assert!(data.needs_representative());

        data.country = Some("NL".parse().unwrap());
        data.place_of_residence = Some(PlaceOfResidence::Known("Kralendijk".to_string()));
        assert!(data.needs_representative());

        // manually typed places are matched case-insensitively
        data.place_of_residence = Some(PlaceOfResidence::Unknown("bonaire".to_string()));
        assert!(data.needs_representative());
    }

    #[test]
    fn missing_bsn_produces_warning() {
        let mut data = complete_personal_data();
        data.bsn = None;
        assert!(
            data.get_problems(ElectionConfig::EK27)
                .potential_problems
                .contains(&PotentialProblems::NoBsn)
        );
    }

    #[test]
    fn missing_date_of_birth_produces_error() {
        let mut data = complete_personal_data();
        data.date_of_birth = None;
        assert!(
            data.get_problems(ElectionConfig::EK27)
                .potential_problems
                .contains(&PotentialProblems::NoDateOfBirth)
        );
    }

    #[test]
    fn very_old_date_of_birth_produces_warning() {
        let mut data = complete_personal_data();
        data.date_of_birth = Some(NaiveDate::from_ymd_opt(1900, 1, 1).unwrap().into());
        assert!(
            data.get_problems(ElectionConfig::EK27)
                .info_problems
                .contains(&InfoProblems::VeryOldDateOfBirth)
        );
    }

    #[test]
    fn too_young_date_of_birth_produces_warning() {
        let store = PgStore::new_for_test();
        let eligible_dob = store.election.eligible_date_of_birth();
        let mut data = complete_personal_data();
        data.date_of_birth = Some((eligible_dob + Duration::days(1)).into());
        assert!(
            data.get_problems(ElectionConfig::EK27)
                .potential_problems
                .contains(&PotentialProblems::TooYoungDateOfBirth)
        );
    }

    #[test]
    fn recent_date_of_birth_does_not_warn() {
        let mut data = complete_personal_data();
        data.date_of_birth = Some("01-01-2000".parse().unwrap());
        assert!(
            !data
                .get_problems(ElectionConfig::EK27)
                .info_problems
                .contains(&InfoProblems::VeryOldDateOfBirth)
        );
    }

    #[test]
    fn missing_place_of_residence_produces_error() {
        let mut data = complete_personal_data();
        data.place_of_residence = None;
        assert!(
            data.get_problems(ElectionConfig::EK27)
                .potential_problems
                .contains(&PotentialProblems::NoPlaceOfResidence)
        );
    }

    #[test]
    fn unknown_place_of_residence_in_nl_produces_warning() {
        let mut data = complete_personal_data();
        data.place_of_residence = Some(PlaceOfResidence::Unknown("Faketown".to_string()));
        data.country = Some("NL".parse().unwrap());
        assert!(
            data.get_problems(ElectionConfig::EK27)
                .potential_problems
                .contains(&PotentialProblems::UnknownPlaceOfResidence)
        );
    }

    #[test]
    fn unknown_place_of_residence_outside_nl_does_not_warn() {
        let mut data = complete_personal_data();
        data.place_of_residence = Some(PlaceOfResidence::Unknown("Faketown".to_string()));
        data.country = Some("DE".parse().unwrap());
        assert!(
            !data
                .get_problems(ElectionConfig::EK27)
                .potential_problems
                .contains(&PotentialProblems::UnknownPlaceOfResidence)
        );
    }

    #[test]
    fn show_unknown_place_of_residence_warning_when_unknown_and_in_nl() {
        let mut data = complete_personal_data();
        data.place_of_residence = Some(PlaceOfResidence::Unknown("Leipzig".to_string()));
        data.country = Some("NL".parse().unwrap());
        assert!(data.show_unknown_place_of_residence_warning());
    }

    #[test]
    fn show_unknown_place_of_residence_warning_false_when_known() {
        let data = complete_personal_data();
        assert!(!data.show_unknown_place_of_residence_warning());
    }

    #[test]
    fn show_unknown_place_of_residence_warning_false_when_unknown_but_abroad() {
        let mut data = complete_personal_data();
        data.place_of_residence = Some(PlaceOfResidence::Unknown("Leipzig".to_string()));
        data.country = Some("DE".parse().unwrap());
        assert!(!data.show_unknown_place_of_residence_warning());
    }

    #[test]
    fn show_unknown_place_of_residence_warning_false_when_no_place() {
        let mut data = complete_personal_data();
        data.place_of_residence = None;
        assert!(!data.show_unknown_place_of_residence_warning());
    }

    #[test]
    fn missing_country_of_residence_produces_error() {
        let mut data = complete_personal_data();
        data.country = None;
        assert!(
            data.get_problems(ElectionConfig::EK27)
                .potential_problems
                .contains(&PotentialProblems::NoCountryOfResidence)
        );
    }
}
