//! The record of one request to the BRP, kept so the audit log shows every
//! consultation: what was sent, for which candidates, and what came back.

use serde::{Deserialize, Serialize};

use super::BrpField;
use crate::structs::{common::Bsn, persons::PersonId};

/// One request to the BRP `personen` endpoint, in the form it is sent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum BrpQuery {
    #[serde(rename = "RaadpleegMetBurgerservicenummer")]
    ConsultWithBsn {
        #[serde(rename = "burgerservicenummer")]
        bsn: Vec<Bsn>,
        fields: Vec<BrpField>,
    },
    /// Search on personal details, for a candidate whose burgerservicenummer
    /// resolves to nobody. The BRP matches `geslachtsnaam` exactly and expects
    /// the prefix separately, and it leaves deceased people out unless asked.
    #[serde(rename = "ZoekMetGeslachtsnaamEnGeboortedatum")]
    SearchByLastNameAndDateOfBirth {
        #[serde(rename = "geslachtsnaam")]
        last_name: String,
        #[serde(rename = "geboortedatum")]
        date_of_birth: String,
        #[serde(rename = "voorvoegsel", skip_serializing_if = "Option::is_none")]
        last_name_prefix: Option<String>,
        #[serde(rename = "geslacht", skip_serializing_if = "Option::is_none")]
        gender: Option<String>,
        #[serde(rename = "inclusiefOverledenPersonen")]
        include_deceased: bool,
        fields: Vec<BrpField>,
    },
}

/// One request sent to the BRP and what it returned. Recorded on the stream as
/// [`crate::CsbAction::BrpLookup`] whether the request succeeded or not, so
/// the audit log holds every time a candidate's data went to the BRP.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrpLookup {
    /// The candidates the request was made for.
    pub persons: Vec<PersonId>,
    pub query: BrpQuery,
    pub outcome: BrpLookupOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BrpLookupOutcome {
    /// The BRP answered with these persons, each by the burgerservicenummer
    /// it returned for them. Kept as the BRP wrote them: a number this
    /// application cannot read is still a person the BRP returned.
    Returned { bsns: Vec<String> },
    /// The request failed, so nothing was compared.
    Failed { error: String },
}
