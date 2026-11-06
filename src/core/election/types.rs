use serde::Serialize;

use crate::Locale;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ElectionType {
    Tk,
    Ek,
    Gr,
    Ps,
    Ws,
    Ep,
    Kc,
    Kcni,
    Er,
}

impl ElectionType {
    /// Short uppercase code identifying the election type (e.g. `"PS"`, `"WS"`).
    pub fn code(&self) -> &'static str {
        match self {
            ElectionType::Tk => "TK",
            ElectionType::Ek => "EK",
            ElectionType::Gr => "GR",
            ElectionType::Ps => "PS",
            ElectionType::Ws => "WS",
            ElectionType::Ep => "EP",
            ElectionType::Kc => "KC",
            ElectionType::Kcni => "KCNI",
            ElectionType::Er => "ER",
        }
    }

    pub fn title(&self, locale: Locale) -> &'static str {
        match (self, locale) {
            (ElectionType::Tk, Locale::En) => "election of the House of Representatives",
            (ElectionType::Tk, Locale::Nl) => "Tweede Kamerverkiezing",
            (ElectionType::Ek, Locale::En) => "election of the Senate",
            (ElectionType::Ek, Locale::Nl) => "Eerste Kamerverkiezing",
            (ElectionType::Gr, Locale::En) => "elections of the municipal council",
            (ElectionType::Gr, Locale::Nl) => "gemeenteraadsverkiezingen",
            (ElectionType::Ps, Locale::En) => "elections of the provincial council",
            (ElectionType::Ps, Locale::Nl) => "Provinciale Statenverkiezingen",
            (ElectionType::Ws, Locale::En) => "elections of the water authority",
            (ElectionType::Ws, Locale::Nl) => "waterschapsverkiezingen",
            (ElectionType::Ep, Locale::En) => "election of the European Parliament",
            (ElectionType::Ep, Locale::Nl) => "Europees Parlementsverkiezing",
            (ElectionType::Kc, Locale::En) => "electoral colleges for the Senate",
            (ElectionType::Kc, Locale::Nl) => "kiescolleges Eerste Kamer",
            (ElectionType::Kcni, Locale::En) => "electoral colleges for non-residents",
            (ElectionType::Kcni, Locale::Nl) => "kiescolleges niet-ingezetenen",
            (ElectionType::Er, Locale::En) => "elections of the Island Councils",
            (ElectionType::Er, Locale::Nl) => "eilandsraadsverkiezingen",
        }
    }

    pub fn body(&self, locale: Locale) -> &'static str {
        match (self, locale) {
            (ElectionType::Tk, Locale::En) => "the House of Representatives",
            (ElectionType::Tk, Locale::Nl) => "de Tweede Kamer",
            (ElectionType::Ek, Locale::En) => "the Senate",
            (ElectionType::Ek, Locale::Nl) => "de Eerste Kamer",
            (ElectionType::Gr, Locale::En) => "the municipal council",
            (ElectionType::Gr, Locale::Nl) => "de gemeenteraad",
            (ElectionType::Ps, Locale::En) => "the provincial council",
            (ElectionType::Ps, Locale::Nl) => "Provinciale Staten",
            (ElectionType::Ws, Locale::En) => "the water authority",
            (ElectionType::Ws, Locale::Nl) => "het waterschap",
            (ElectionType::Ep, Locale::En) => "the European Parliament",
            (ElectionType::Ep, Locale::Nl) => "het Europees Parlement",
            (ElectionType::Kc, Locale::En) | (ElectionType::Kcni, Locale::En) => {
                "the electoral college"
            }
            (ElectionType::Kc, Locale::Nl) | (ElectionType::Kcni, Locale::Nl) => "het kiescollege",
            (ElectionType::Er, Locale::En) => "the island council",
            (ElectionType::Er, Locale::Nl) => "de eilandsraad",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn election_type_title_match() {
        for election_type in &[
            ElectionType::Tk,
            ElectionType::Ek,
            ElectionType::Gr,
            ElectionType::Ps,
            ElectionType::Ws,
            ElectionType::Ep,
            ElectionType::Er,
        ] {
            assert!(election_type.title(Locale::Nl).contains("verkiezing"));
            assert!(election_type.title(Locale::En).contains("election"));
        }

        // Kc uses "kiescolleges" rather than "verkiezing"
        assert!(ElectionType::Kc.title(Locale::Nl).contains("kiescolleges"));
        assert!(
            ElectionType::Kc
                .title(Locale::En)
                .contains("electoral colleges")
        );
    }

    #[test]
    fn election_type_codes_are_two_uppercase_letters() {
        let cases = [
            (ElectionType::Tk, "TK"),
            (ElectionType::Ek, "EK"),
            (ElectionType::Gr, "GR"),
            (ElectionType::Ps, "PS"),
            (ElectionType::Ws, "WS"),
            (ElectionType::Ep, "EP"),
            (ElectionType::Kc, "KC"),
            (ElectionType::Er, "ER"),
        ];
        for (election_type, expected) in cases {
            assert_eq!(election_type.code(), expected);
        }
    }
}
