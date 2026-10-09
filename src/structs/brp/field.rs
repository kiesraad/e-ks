use serde::{Deserialize, Serialize};

/// A field of the BRP `personen` endpoint, named by the dotted path the API
/// expects. [`super::client::CANDIDATE_FIELDS`] lists the ones we request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BrpField {
    // Personen
    #[serde(rename = "burgerservicenummer")]
    Bsn,
    #[serde(rename = "naam.voornamen")]
    FirstNames,
    #[serde(rename = "naam.voorletters")]
    Initials,
    #[serde(rename = "naam.adellijkeTitelPredicaat")]
    TitleOfNobility,
    #[serde(rename = "naam.voorvoegsel")]
    LastNamePrefix,
    #[serde(rename = "naam.geslachtsnaam")]
    LastName,
    #[serde(rename = "geboorte.datum")]
    DateOfBirth,
    #[serde(rename = "geslacht")]
    Gender,
    #[serde(rename = "naam.aanduidingNaamgebruik")]
    DesignatedNameUsage,

    // Eligibility to stand for election (article 56 Grondwet).
    #[serde(rename = "overlijden.datum")]
    DateOfDeath,
    #[serde(rename = "nationaliteiten.nationaliteit")]
    Nationality,
    #[serde(rename = "uitsluitingKiesrecht")]
    SuffrageExclusion,

    // Only the `woonplaats` is verified: it is the one address element
    // printed on the candidate list.
    #[serde(rename = "verblijfplaats.verblijfadres.woonplaats")]
    PlaceOfResidence,

    // Municipality of registration
    // TODO: What to do with Registratie Niet Ingezetenen?
    #[serde(rename = "gemeenteVanInschrijving")]
    RegisteredMunicipality,
    #[serde(rename = "datumInschrijvingInGemeente")]
    DateMunicipalRegistration,

    // Partners
    #[serde(rename = "partners.naam.voorvoegsel")]
    PartnerLastNamePrefix,
    #[serde(rename = "partners.naam.geslachtsnaam")]
    PartnerLastName,
    #[serde(rename = "partners.aangaanHuwelijkPartnerschap.datum")]
    DateOfMarriage,
    #[serde(rename = "partners.ontbindingHuwelijkPartnerschap")]
    DateOfDissolutionMarriage,
}

impl BrpField {
    /// The dotted path the BRP API knows this field by, for showing a request
    /// as it was sent.
    pub fn api_name(self) -> String {
        serde_json::to_value(self)
            .ok()
            .and_then(|value| value.as_str().map(str::to_string))
            .unwrap_or_default()
    }
}
