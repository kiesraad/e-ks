use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::structs::{
    brp::{BrpFinding, BrpFindingKind},
    persons::PersonId,
};

/// What the BRP answered about the fixture candidates, generated from the
/// `personen-mock` by `brp_findings_are_what_the_mock_answers` rather than
/// written by hand.
const BRP_FINDINGS_JSON: &str = include_str!("brp_findings.json");

/// The findings for one fixture candidate.
#[derive(Debug, Serialize, Deserialize)]
struct CandidateFindings {
    /// The candidate's name, so the file can be read without resolving ids.
    candidate: String,
    person: PersonId,
    findings: Vec<BrpFindingKind>,
}

/// The recorded BRP findings per fixture candidate, holding only the
/// candidates the BRP had something to say about: it agreed with the rest.
pub fn brp_findings() -> HashMap<PersonId, Vec<BrpFinding>> {
    records()
        .into_iter()
        .map(|record| {
            (
                record.person,
                record.findings.into_iter().map(BrpFinding::from).collect(),
            )
        })
        .collect()
}

fn records() -> Vec<CandidateFindings> {
    serde_json::from_str(BRP_FINDINGS_JSON).expect("fixture BRP findings")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        PgStore,
        pagination::SortDirection,
        structs::persons::{Person, PersonSort},
    };

    async fn fixture_persons() -> Vec<Person> {
        let store = PgStore::new_for_test();
        crate::fixtures::persons::load(&store).await.unwrap();

        Person::list(&store, 1000, 0, &PersonSort::LastName, &SortDirection::Asc)
            .unwrap()
            .into_iter()
            .map(|person| person.data)
            .collect()
    }

    #[tokio::test]
    async fn every_finding_belongs_to_a_fixture_candidate() {
        let persons = fixture_persons().await;
        let records = records();

        assert!(!records.is_empty());
        for record in &records {
            let person = persons
                .iter()
                .find(|person| person.id == record.person)
                .unwrap_or_else(|| panic!("no fixture candidate {}", record.candidate));
            assert_eq!(person.name.display(), record.candidate);
            assert!(!record.findings.is_empty());
        }
    }

    /// Regenerate `brp_findings.json` from the running mock, so the fixture
    /// holds the BRP's own answer about the fixture candidates rather than an
    /// invented one. A file that no longer matches is rewritten and the test
    /// fails, leaving the difference in the working tree.
    ///
    /// Run with `docker compose up -d personen-mock` and
    /// `cargo test -- --ignored brp`.
    #[tokio::test]
    #[ignore = "requires the personen-mock container: docker compose up -d personen-mock"]
    async fn brp_findings_are_what_the_mock_answers() {
        use std::{fs, path::PathBuf};

        use crate::structs::brp::{BRP_BSN_BATCH_SIZE, BrpClient};

        let persons = fixture_persons().await;
        let client = BrpClient::new_for_test("http://localhost:5010");

        let mut generated = Vec::new();
        for batch in persons.chunks(BRP_BSN_BATCH_SIZE) {
            for (person_id, findings) in client.verify_batch(batch).await.expect("the mock answers")
            {
                if findings.is_empty() {
                    continue;
                }
                let person = batch
                    .iter()
                    .find(|person| person.id == person_id)
                    .expect("a checked candidate of this batch");
                generated.push(CandidateFindings {
                    candidate: person.name.display(),
                    person: person_id,
                    findings: findings.into_iter().map(|finding| finding.kind).collect(),
                });
            }
        }

        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/fixtures/brp_findings.json");
        let json = format!(
            "{}\n",
            serde_json::to_string_pretty(&generated).expect("findings as JSON")
        );
        if json != fs::read_to_string(&path).expect("the committed findings") {
            fs::write(&path, &json).expect("write the findings");
            panic!("brp_findings.json was out of date and has been rewritten from the mock");
        }
    }
}
