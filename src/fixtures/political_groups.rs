use crate::{
    AppError, PgStore,
    structs::{
        common::{Address, Appellation, DutchAddress, FullName},
        list_designation::ListDesignation,
        list_submitters::{ListSubmitter, ListSubmitterId},
        name_authorisations::{NameAuthorisation, NameAuthorisationId},
        political_groups::PoliticalGroup,
    },
};
use uuid::Uuid;

/// A stable fixture ID derived from `key`.
fn fixture_id<T: From<Uuid>>(key: &[u8]) -> T {
    Uuid::new_v5(&Uuid::NAMESPACE_OID, key).into()
}

/// A `FullName` without first name, parsed from constant fixture values.
fn name(initials: &str, prefix: Option<&str>, last_name: &str) -> FullName {
    FullName {
        first_name: None,
        last_name: last_name.parse().expect("last name"),
        last_name_prefix: prefix.map(|p| p.parse().expect("last name prefix")),
        initials: initials.parse().expect("initials"),
    }
}

/// A Dutch address parsed from constant fixture values.
fn address(
    street: &str,
    number: &str,
    addition: Option<&str>,
    postal_code: &str,
    locality: &str,
) -> Address {
    Address::Dutch(DutchAddress {
        locality: Some(locality.parse().expect("locality")),
        postal_code: Some(postal_code.parse().expect("postal code")),
        house_number: Some(number.parse().expect("house number")),
        house_number_addition: addition.map(|a| a.parse().expect("house number addition")),
        street_name: Some(street.parse().expect("street name")),
        known_in_bag: Some(true),
    })
}

/// The demo political group, named `appellation` when given.
pub fn fixture_group(appellation: Option<Appellation>) -> PoliticalGroup {
    PoliticalGroup {
        appellation: Some(
            appellation.unwrap_or_else(|| "Kiesraad Demo".parse().expect("appellation")),
        ),
        list_designation: Some(ListDesignation::Standalone),
        previous_election_results: None,
    }
}

pub async fn load(store: &PgStore, political_group: PoliticalGroup) -> Result<(), AppError> {
    political_group.update(store).await?;

    NameAuthorisation {
        id: fixture_id::<NameAuthorisationId>(b"fixture_authorised_agent"),
        name: name("A.B.", Some("de"), "Jansen"),
        legal_name: "Kiesraad Demo Partij".parse().expect("legal name"),
    }
    .create(store)
    .await?;

    ListSubmitter {
        id: fixture_id::<ListSubmitterId>(b"fixture_list_submitter"),
        name: name("E.F.", None, "Bos"),
        address: address("Coolsingel", "5", Some("B"), "3011 CC", "Rotterdam"),
        is_substitute: false,
    }
    .update(store)
    .await?;

    ListSubmitter {
        id: fixture_id::<ListSubmitterId>(b"fixture_substitute_submitter_1"),
        name: name("G.H.", Some("van"), "Smit"),
        address: address("Spui", "18", None, "2511 DD", "Den Haag"),
        is_substitute: true,
    }
    .create_substitute(store)
    .await?;

    ListSubmitter {
        id: fixture_id::<ListSubmitterId>(b"fixture_substitute_submitter_2"),
        name: name("I.J.", None, "Jong"),
        address: address("Oudegracht", "21", Some("C"), "3511 AA", "Utrecht"),
        is_substitute: true,
    }
    .create_substitute(store)
    .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::structs::common::{HasSeverity, Problematic};

    #[tokio::test]
    async fn test_load() {
        let store = PgStore::new_for_test();
        load(&store, fixture_group(None)).await.unwrap();

        let list_submitter = store.get_list_submitter();
        assert!(list_submitter.get_problems(()).is_all_good());

        let substitute_submitters = store.get_substitute_submitters();
        assert_eq!(substitute_submitters.len(), 2);
    }
}
