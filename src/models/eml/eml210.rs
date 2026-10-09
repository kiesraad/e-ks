//! The EML 210 candidate nomination export, built with [`eml_nl`].

use eml_nl::{
    common::{
        AuthorityIdentifier, CreatedByAuthority, ListData, ListDataContest, ManagingAuthority,
    },
    documents::{
        EML, ElectionIdentifierBuilder,
        nomination::{
            Nomination, NominationAffiliation, NominationContestIdentifier, NominationNominate,
        },
    },
    io::EMLWrite,
    utils::{AffiliationType, AuthorityId, ContestId, StringValue},
};

use crate::{
    AppError, ElectionConfig, PgStore,
    core::ModelLocale,
    models::eml::{agent, candidate_identifier, contact},
    structs::{
        candidate_lists::{CandidateList, CandidateListId, FullCandidateList},
        candidates::Candidate,
        common::BsnOrNoneConfirmed,
        list_submitters::ListSubmitter,
        political_groups::PoliticalGroup,
    },
};

impl TryInto<eml_nl::documents::nomination::NominationCandidate> for &Candidate {
    type Error = AppError;

    fn try_into(self) -> Result<eml_nl::documents::nomination::NominationCandidate, Self::Error> {
        Ok(eml_nl::documents::nomination::NominationCandidate {
            identifier: candidate_identifier(self.position)?,
            full_name: (&self.person.name).into(),
            date_of_birth: self
                .person
                .personal_data
                .date_of_birth
                .as_ref()
                .map(|n| StringValue::from_value((**n).into())),
            gender: Some(StringValue::from_value((&self.person.personal_data).into())),
            gender_annex: None,
            qualifying_address: (&self.person.personal_data).try_into()?,
            contact: contact(&self.person),
            agent: agent(&self.person),
            date_of_birth_annex: None,
            national_identification_number: match self.person.personal_data.bsn.as_ref() {
                Some(BsnOrNoneConfirmed::Bsn(bsn)) => Some(bsn.to_exposed_string().into()),
                _ => None,
            },
        })
    }
}

fn nomination_proposer(
    submitter: ListSubmitter,
    job_title: eml_nl::documents::nomination::NominationJobTitle,
    id: Option<Box<str>>,
) -> Result<eml_nl::documents::nomination::NominationProposer, AppError> {
    Ok(eml_nl::documents::nomination::NominationProposer {
        name: (&submitter.name).into(),
        contact: (&submitter.address).into(),
        job_title: StringValue::Parsed(job_title),
        id,
        living_address: None,
    })
}

/// The list submitter and its deputies as nomination proposers
fn nominated_proposers(
    store: &PgStore,
) -> Result<Vec<eml_nl::documents::nomination::NominationProposer>, AppError> {
    let substitutes = store.get_substitute_submitters();
    let mut nominated = Vec::with_capacity(1 + substitutes.len());
    nominated.push(nomination_proposer(
        store.get_list_submitter(),
        eml_nl::documents::nomination::NominationJobTitle::Submitter,
        None,
    )?);

    for (i, sub) in substitutes.into_iter().enumerate() {
        nominated.push(nomination_proposer(
            sub,
            eml_nl::documents::nomination::NominationJobTitle::DeputySubmitter,
            Some((i + 1).to_string().into()),
        )?);
    }

    Ok(nominated)
}

// ListData is additional data specifically for OSV, we can possibly change this in the future if necessary
fn list_data(list: &CandidateList, locale: ModelLocale) -> Result<ListData, AppError> {
    Ok(ListData {
        // We always publish genders, but the individual candidates may leave the gender unspecified
        publish_gender: StringValue::Parsed(true),
        publication_language: Some(StringValue::from_value(match locale {
            ModelLocale::Fry => eml_nl::utils::PublicationLanguage::Frisian,
            ModelLocale::Nl => eml_nl::utils::PublicationLanguage::Dutch,
        })),
        belongs_to_set: None,
        belongs_to_combination: None,
        contests: list
            .electoral_districts
            .iter()
            .map(|d| {
                let id = ContestId::new(d.region_number().to_string())?;
                Ok(ListDataContest::new(id).with_name(d.title()))
            })
            .collect::<Result<Vec<ListDataContest>, AppError>>()?,
    })
}

/// Build the EML 210 candidate nomination XML for a candidate list
pub fn eml210(
    store: &PgStore,
    election: &ElectionConfig,
    political_group: &PoliticalGroup,
    list_id: CandidateListId,
    locale: ModelLocale,
) -> Result<Vec<u8>, AppError> {
    let FullCandidateList { list, candidates } = FullCandidateList::get(store, list_id)?;

    let nominated = nominated_proposers(store)?;
    let list_data = list_data(&list, locale)?;

    let now = chrono::Utc::now();
    let nomination = Nomination::builder()
        .transaction_id(
            u64::try_from(store.current_event_id()).map_err(|_| AppError::InternalServerError)?,
        )
        .managing_authority(
            ManagingAuthority::new(AuthorityIdentifier::new(AuthorityId::new("0000")?))
                .with_created_by_authority(
                    CreatedByAuthority::new(AuthorityId::new("0000")?)
                        .with_name("De politieke partij"),
                ),
        )
        .issue_date(now.date_naive())
        .creation_date_time(now)
        .election_identifier(
            ElectionIdentifierBuilder::try_from(*election)?.build_for_nomination()?,
        )
        .contest_identifier(if election.has_only_one_district() {
            NominationContestIdentifier::new(ContestId::geen(), "")
        } else if list.contains_all_districts(election) {
            NominationContestIdentifier::new(ContestId::alle(), "")
        } else {
            // If there are multiple districts but this list is not linked to all districts,
            // we always choose the first district (to avoid collisions with other lists).
            // The full set of electoral districts can be found in the ListData.
            let district = list
                .electoral_districts
                .first()
                .ok_or(AppError::InternalServerError)?;
            NominationContestIdentifier::new(
                ContestId::new(district.region_number().to_string())?,
                district.title(),
            )
        })
        .affiliation(NominationAffiliation {
            registered_name: political_group.pg_appellation()?.into(),
            affiliation_type: StringValue::from_value(AffiliationType::StandAloneList),
            list_data,
            candidates: candidates
                .iter()
                .map(|c| (&c.data).try_into())
                .collect::<Result<Vec<_>, AppError>>()?,
        })
        .nominate(NominationNominate::new(nominated))
        .build()?;

    Ok(EML::from_nomination_doc(nomination).write_eml_root(true, true)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{assert_matches, collections::BTreeSet, str::FromStr};

    use crate::{
        AppError, Context, ElectoralDistrict, PgStore,
        core::ModelLocale,
        models::eml::remove_variable_fields,
        structs::{
            candidate_lists::{CandidateListId, FullCandidateList},
            common::CountryCode,
            list_submitters::ListSubmitterId,
            persons::{PersonId, Representative},
        },
        test_utils::{
            sample_candidate_list, sample_dutch_address, sample_full_name, sample_list_submitter,
            sample_person,
        },
    };

    async fn create_sample_list(store: &PgStore) -> Result<FullCandidateList, AppError> {
        let list_id = CandidateListId::new();
        let mut list = sample_candidate_list(list_id);

        let person_id1 = PersonId::new();
        let mut sample_person1 = sample_person(person_id1);
        sample_person1.name.last_name = "Candidate I".parse().unwrap();
        sample_person1.create(store).await?;
        list.candidates.push(person_id1);

        let person_id2 = PersonId::new();
        let mut sample_person2 = sample_person(person_id2);
        sample_person2.name.last_name = "Candidate II".parse().unwrap();
        sample_person2.personal_data.bsn = Some("999995972".parse().unwrap());
        sample_person2.personal_data.country = CountryCode::from_str("BE").ok();
        sample_person2.personal_data.gender = None;

        sample_person2.representative = Some(Representative {
            name: sample_full_name(Some("Bob"), "Bouwer", Some("de"), "B."),
            address: sample_dutch_address("Nijmegen", "1234AB", "22", "c", "Bouwstraat"),
        });
        sample_person2.create(store).await?;
        list.candidates.push(person_id2);

        let mut submitter = sample_list_submitter(ListSubmitterId::new());
        submitter.name.last_name = "Submitter".parse().unwrap();
        submitter.update(store).await?;

        let mut sub_submitter1 = sample_list_submitter(ListSubmitterId::new());
        sub_submitter1.name.last_name = "Sub Submitter I".parse().unwrap();
        let mut sub_submitter2 = sample_list_submitter(ListSubmitterId::new());
        sub_submitter2.name.last_name = "Sub Submitter II".parse().unwrap();
        sub_submitter1.create_substitute(store).await?;
        sub_submitter2.create_substitute(store).await?;

        list.create(store).await?;

        FullCandidateList::get(store, list_id)
    }

    async fn check_eml(response: &str, expected: &str) {
        // should parse
        assert_matches!(response.parse().unwrap(), EML::Nomination(_));
        assert_matches!(expected.parse().unwrap(), EML::Nomination(_));

        // should match the expected document, other than the variable fields
        assert_eq!(
            remove_variable_fields(response),
            remove_variable_fields(expected),
            "received XML:\n{}",
            response
        );
    }

    #[tokio::test]
    async fn ek_export() {
        // setup
        let store = PgStore::new_for_test();
        let mut context = Context::new_test_without_db();
        context.election = ElectionConfig::EK27;
        let list = create_sample_list(&store).await.unwrap();

        // test
        let eml = eml210(
            &store,
            &context.election,
            &store.get_political_group(),
            list.id(),
            ModelLocale::Nl,
        )
        .unwrap();

        // verify
        check_eml(
            &String::from_utf8(eml).unwrap(),
            include_str!("testdata/210-ek27.eml.xml"),
        )
        .await;
    }

    #[tokio::test]
    async fn ps1_export() {
        // setup
        let store = PgStore::new_for_test();
        let mut context = Context::new_test_without_db();
        context.election = ElectionConfig::PS27(crate::Province::Groningen);
        let mut list = create_sample_list(&store).await.unwrap();
        list.list.electoral_districts = BTreeSet::from([ElectoralDistrict::PsGroningen]);
        list.list.update_districts(&store).await.unwrap();

        // test
        let eml = eml210(
            &store,
            &context.election,
            &store.get_political_group(),
            list.id(),
            ModelLocale::Nl,
        )
        .unwrap();

        // verify
        check_eml(
            &String::from_utf8(eml).unwrap(),
            include_str!("testdata/210-ps27-1.eml.xml"),
        )
        .await;
    }

    #[tokio::test]
    async fn ps2_export() {
        // setup
        let store = PgStore::new_for_test();
        let mut context = Context::new_test_without_db();
        context.election = ElectionConfig::PS27(crate::Province::Limburg);
        let mut list = create_sample_list(&store).await.unwrap();
        list.list.electoral_districts =
            BTreeSet::from([ElectoralDistrict::PsMaastricht, ElectoralDistrict::PsVenlo]);
        list.list.update_districts(&store).await.unwrap();

        // test
        let eml = eml210(
            &store,
            &context.election,
            &store.get_political_group(),
            list.id(),
            ModelLocale::Nl,
        )
        .unwrap();

        // verify
        check_eml(
            &String::from_utf8(eml).unwrap(),
            include_str!("testdata/210-ps27-2.eml.xml"),
        )
        .await;
    }

    #[tokio::test]
    async fn ws_export() {
        // setup
        let store = PgStore::new_for_test();
        let mut context = Context::new_test_without_db();
        context.election = ElectionConfig::WS27(crate::WaterCouncil::Fryslan);
        let mut list = create_sample_list(&store).await.unwrap();
        list.list.electoral_districts = BTreeSet::from([ElectoralDistrict::WsFryslan]);
        list.list.update_districts(&store).await.unwrap();

        // test
        let eml = eml210(
            &store,
            &context.election,
            &store.get_political_group(),
            list.id(),
            ModelLocale::Fry,
        )
        .unwrap();

        // verify
        check_eml(
            &String::from_utf8(eml).unwrap(),
            include_str!("testdata/210-ws27.eml.xml"),
        )
        .await;
    }
}
