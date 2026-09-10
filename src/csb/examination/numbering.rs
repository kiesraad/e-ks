//! The numbering of the candidate lists (Kieswet Art. I 14). The lists of the
//! political groups that obtained one or more seats at the previous election
//! come first, in the order of their votes; the remaining lists follow in the
//! order drawn by lot during the public session. That is the starting order
//! on the finalise page, where the committee records the final order.

use crate::{
    AppError, CsbMainStore, CsbStoreData, StreamId,
    csb::examination::extractors::CsbPoliticalGroup, store::StoreRegistry,
    structs::csb::RegisteredPoliticalGroup,
};

/// One political group in the numbering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NumberedGroup {
    pub stream_id: StreamId,
    pub appellation: String,
    /// The list number; `None` while the order is not recorded yet and the
    /// group is not numbered on votes.
    pub position: Option<usize>,
    /// Whether the group obtained a seat at the previous election, which
    /// numbers its lists on votes (the I 4's first numbering table).
    pub numbered_on_votes: bool,
    /// Votes at the previous election; `None` for an unregistered group.
    pub previous_votes: Option<u64>,
    /// Seats at the previous election; `None` for an unregistered group.
    pub previous_seats: Option<u32>,
    /// The districts the group still has a valid list in.
    pub district_count: usize,
}

/// The numbering of every political group with a valid list.
#[derive(Debug, Default)]
pub struct ListNumbering {
    /// In list order: the recorded order, followed by the groups not in it in
    /// their starting order (on votes, then most districts first).
    pub groups: Vec<NumberedGroup>,
}

impl ListNumbering {
    /// Numbers `groups`, matching them to the `registered` groups on
    /// appellation and ordering them by the recorded `list_order`.
    pub fn new(
        groups: &[CsbPoliticalGroup],
        registered: &[RegisteredPoliticalGroup],
        list_order: &[StreamId],
    ) -> Self {
        let mut numbered: Vec<(Option<&RegisteredPoliticalGroup>, NumberedGroup)> = groups
            .iter()
            .filter(|group| !group.is_deleted)
            .filter_map(|group| {
                let district_count = group.valid_districts().len();
                if district_count == 0 {
                    return None;
                }
                let registration = registration(group, registered);
                Some((
                    registration,
                    NumberedGroup {
                        stream_id: group.stream_id,
                        appellation: group.numbering_appellation(),
                        position: None,
                        numbered_on_votes: registration
                            .is_some_and(RegisteredPoliticalGroup::is_numbered_on_votes),
                        previous_votes: registration.map(|r| r.previous_votes.value()),
                        previous_seats: registration.map(|r| r.previous_seats.value()),
                        district_count,
                    },
                ))
            })
            .collect();

        // The starting order: on votes first, then most districts first.
        numbered.sort_by(|(reg_a, a), (reg_b, b)| {
            b.numbered_on_votes.cmp(&a.numbered_on_votes).then_with(|| {
                match (a.numbered_on_votes, reg_a, reg_b) {
                    (true, Some(reg_a), Some(reg_b)) => reg_a.numbering_order(reg_b),
                    _ => b
                        .district_count
                        .cmp(&a.district_count)
                        .then_with(|| a.appellation.cmp(&b.appellation)),
                }
            })
        });
        let mut remaining: Vec<NumberedGroup> = numbered.into_iter().map(|(_, g)| g).collect();

        let mut ordered: Vec<NumberedGroup> = list_order
            .iter()
            .filter_map(|stream_id| {
                remaining
                    .iter()
                    .position(|group| group.stream_id == *stream_id)
                    .map(|index| remaining.remove(index))
            })
            .collect();
        ordered.append(&mut remaining);

        // Without a recorded order only the groups numbered on votes have
        // their number; the I 4 leaves the others blank for the session.
        let recorded = !list_order.is_empty();
        for (index, group) in ordered.iter_mut().enumerate() {
            if recorded || group.numbered_on_votes {
                group.position = Some(index + 1);
            }
        }

        Self { groups: ordered }
    }

    /// The groups numbered on votes, in list order.
    pub fn on_votes(&self) -> impl Iterator<Item = &NumberedGroup> {
        self.groups.iter().filter(|group| group.numbered_on_votes)
    }

    /// The groups numbered by lot, in list order.
    pub fn by_lot(&self) -> impl Iterator<Item = &NumberedGroup> {
        self.groups.iter().filter(|group| !group.numbered_on_votes)
    }

    /// The streams of the numbered groups, in their current order.
    pub fn stream_ids(&self) -> Vec<StreamId> {
        self.groups.iter().map(|group| group.stream_id).collect()
    }
}

/// The registration matching the group's appellation, if any.
fn registration<'a>(
    group: &CsbPoliticalGroup,
    registered: &'a [RegisteredPoliticalGroup],
) -> Option<&'a RegisteredPoliticalGroup> {
    let appellation = group.registered_appellation()?;
    registered
        .iter()
        .find(|registration| registration.has_appellation(appellation))
}

/// The numbering of the election's imported groups, as recorded on the main
/// store.
pub async fn list_numbering(
    registry: &StoreRegistry<CsbStoreData>,
    main_store: &CsbMainStore,
) -> Result<ListNumbering, AppError> {
    let groups: Vec<CsbPoliticalGroup> = registry
        .stores_for_election(main_store.election)
        .await?
        .iter()
        .map(CsbPoliticalGroup::new_from_csb_store)
        .collect();

    Ok(ListNumbering::new(
        &groups,
        &main_store.registered_political_groups(),
        &main_store.list_order(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    use crate::{
        ElectoralDistrict,
        csb::examination::structs::BrpCheckState,
        structs::{
            candidate_lists::CandidateListId,
            csb::{CsbPhase, sample_registered_political_group},
            list_designation::ListDesignation,
            political_groups::PoliticalGroup,
        },
    };

    /// An undeleted group named `appellation` with one list in `districts`.
    fn group(appellation: &str, districts: Vec<ElectoralDistrict>) -> CsbPoliticalGroup {
        CsbPoliticalGroup {
            political_group: PoliticalGroup {
                appellation: Some(appellation.parse().unwrap()),
                list_designation: Some(ListDesignation::Standalone),
                ..Default::default()
            },
            stream_id: StreamId::new(),
            brp: BrpCheckState::NotChecked,
            mode: CsbPhase::Examination,
            is_examination_finished: true,
            is_deleted: false,
            scrapped: Default::default(),
            restoration_count: 0,
            omission_count: 0,
            recovery: Default::default(),
            first_candidate_name: None,
            candidate_list_districts: HashMap::from([(CandidateListId::new(), districts)]),
        }
    }

    fn names(groups: &[NumberedGroup]) -> Vec<(&str, Option<usize>)> {
        groups
            .iter()
            .map(|group| (group.appellation.as_str(), group.position))
            .collect()
    }

    #[test]
    fn seated_registered_groups_start_first_most_votes_first() {
        let groups = [
            group("Klein", vec![ElectoralDistrict::Groningen]),
            group("groot", vec![ElectoralDistrict::Groningen]),
            group("Nieuw", vec![ElectoralDistrict::Groningen]),
            group("Ongeregistreerd", vec![ElectoralDistrict::Groningen]),
        ];
        let registered = [
            sample_registered_political_group("Klein", 100, 1),
            // Matched ignoring case.
            sample_registered_political_group("Groot", 5000, 7),
            // Registered without a seat: numbered by lot, votes still shown.
            sample_registered_political_group("Nieuw", 50, 0),
        ];

        let numbering = ListNumbering::new(&groups, &registered, &[]);

        assert_eq!(
            names(&numbering.groups),
            [
                ("groot", Some(1)),
                ("Klein", Some(2)),
                ("Nieuw", None),
                ("Ongeregistreerd", None)
            ]
        );
        let results: Vec<_> = numbering
            .groups
            .iter()
            .map(|g| (g.numbered_on_votes, g.previous_votes, g.previous_seats))
            .collect();
        assert_eq!(
            results,
            [
                (true, Some(5000), Some(7)),
                (true, Some(100), Some(1)),
                (false, Some(50), Some(0)),
                (false, None, None)
            ]
        );
        assert_eq!(numbering.on_votes().count(), 2);
        assert_eq!(numbering.by_lot().count(), 2);
    }

    #[test]
    fn unrecorded_lot_order_is_most_districts_first_then_alphabetical() {
        let groups = [
            group("Beta", vec![ElectoralDistrict::Groningen]),
            group(
                "Alpha",
                vec![ElectoralDistrict::Groningen, ElectoralDistrict::Drenthe],
            ),
            group("Gamma", vec![ElectoralDistrict::Groningen]),
        ];

        let numbering = ListNumbering::new(&groups, &[], &[]);

        assert_eq!(
            names(&numbering.groups),
            [("Alpha", None), ("Beta", None), ("Gamma", None)]
        );
        assert_eq!(numbering.groups[0].district_count, 2);
    }

    /// The recorded order wins over the starting order, seated groups
    /// included; every group then has its number.
    #[test]
    fn recorded_order_numbers_every_group() {
        let seated = group("Zetel", vec![ElectoralDistrict::Groningen]);
        let first = group("Eerste", vec![ElectoralDistrict::Groningen]);
        let second = group("Tweede", vec![ElectoralDistrict::Groningen]);
        let order = [second.stream_id, seated.stream_id, first.stream_id];
        let registered = [sample_registered_political_group("Zetel", 100, 1)];

        let numbering = ListNumbering::new(&[seated, first, second], &registered, &order);

        assert_eq!(
            names(&numbering.groups),
            [("Tweede", Some(1)), ("Zetel", Some(2)), ("Eerste", Some(3))]
        );
        assert_eq!(numbering.stream_ids(), order);
        assert_eq!(
            names(&numbering.on_votes().cloned().collect::<Vec<_>>()),
            [("Zetel", Some(2))]
        );
    }

    #[test]
    fn groups_missing_from_the_recorded_order_follow_it() {
        let recorded = group("Opgenomen", vec![ElectoralDistrict::Groningen]);
        let added = group(
            "Nieuw",
            vec![ElectoralDistrict::Groningen, ElectoralDistrict::Drenthe],
        );
        let order = [StreamId::new(), recorded.stream_id];

        let numbering = ListNumbering::new(&[added, recorded], &[], &order);

        assert_eq!(
            names(&numbering.groups),
            [("Opgenomen", Some(1)), ("Nieuw", Some(2))]
        );
    }

    #[test]
    fn deleted_groups_and_groups_without_a_valid_list_are_not_numbered() {
        let deleted = CsbPoliticalGroup {
            is_deleted: true,
            ..group("Weg", vec![ElectoralDistrict::Groningen])
        };
        let without_lists = CsbPoliticalGroup {
            candidate_list_districts: HashMap::new(),
            ..group("Leeg", vec![])
        };
        let registered = [sample_registered_political_group("Weg", 100, 1)];

        let numbering = ListNumbering::new(&[deleted, without_lists], &registered, &[]);

        assert!(numbering.groups.is_empty());
    }

    #[test]
    fn blank_lists_are_numbered_by_lot_even_when_registered() {
        let blank = CsbPoliticalGroup {
            political_group: PoliticalGroup {
                appellation: Some("Blanco Partij".parse().unwrap()),
                list_designation: Some(ListDesignation::Blank),
                ..Default::default()
            },
            ..group("Blanco Partij", vec![ElectoralDistrict::Groningen])
        };
        let registered = [sample_registered_political_group("Blanco Partij", 100, 1)];

        let numbering = ListNumbering::new(&[blank], &registered, &[]);

        assert_eq!(names(&numbering.groups), [("Blanco", None)]);
        assert!(!numbering.groups[0].numbered_on_votes);
        assert_eq!(numbering.groups[0].previous_votes, None);
        assert_eq!(numbering.groups[0].previous_seats, None);
    }
}
