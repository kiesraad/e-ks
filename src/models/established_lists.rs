//! A political group's established lists and their sets of equal
//! ("gelijkluidende") lists, shared by the EML 230b export and the candidate
//! list overview so both classify and number them the same way.

use std::num::NonZeroU64;

use eml_nl::{
    common::{ListData, ListDataContest},
    utils::{AffiliationType, ContestId},
};

use crate::{
    AppError, CsbStream, ElectoralDistrict,
    models::csb_model_inputs::valid_lists_by_district,
    projection::Scrapped,
    structs::{candidate_lists::CandidateList, persons::PersonId},
};

/// A group's sets of equal lists, ordered on their lowest district; never
/// empty
///
/// A set is the [`ListData`] of the lists in it: `belongs_to_set` numbers the
/// sets in more than one district, and `contests` are its districts, by
/// region number in region number order.
#[derive(Debug, Clone)]
pub struct ListSets(Vec<ListData>);

impl ListSets {
    /// `None` without any district
    pub fn new(
        batches: impl IntoIterator<Item = Vec<ElectoralDistrict>>,
    ) -> Result<Option<Self>, AppError> {
        let mut batches: Vec<Vec<ElectoralDistrict>> = batches
            .into_iter()
            .filter(|districts| !districts.is_empty())
            .map(|mut districts| {
                districts.sort_by_key(ElectoralDistrict::region_number);
                districts
            })
            .collect();
        if batches.is_empty() {
            return Ok(None);
        }
        batches.sort_by_key(|districts| districts.first().map(ElectoralDistrict::region_number));

        let mut next = NonZeroU64::MIN;
        let mut sets = Vec::with_capacity(batches.len());
        for districts in batches {
            let mut set = ListData::new(true);
            if districts.len() > 1 {
                set = set.with_belongs_to_set(next);
                next = next.saturating_add(1);
            }
            for district in districts {
                set.contests.push(contest(district)?);
            }
            sets.push(set);
        }

        Ok(Some(Self(sets)))
    }

    pub fn iter(&self) -> impl Iterator<Item = &ListData> {
        self.0.iter()
    }

    pub fn affiliation_type(&self) -> AffiliationType {
        match self.0.as_slice() {
            [set] if set.contests.len() == 1 => AffiliationType::StandAloneList,
            [_] => AffiliationType::SetOfEqualLists,
            _ => AffiliationType::GroupOfLists,
        }
    }

    pub fn contains(&self, district: &ElectoralDistrict) -> bool {
        self.set(district).is_some()
    }

    /// The number of the set `district` belongs to
    pub fn set_number(&self, district: &ElectoralDistrict) -> Option<NonZeroU64> {
        self.set(district).and_then(set_number)
    }

    fn set(&self, district: &ElectoralDistrict) -> Option<&ListData> {
        let region = district.region_number().to_string();
        self.0.iter().find(|set| {
            set.contests
                .iter()
                .any(|contest| contest.id.raw() == region)
        })
    }
}

/// The district as a contest, like the 230b identifies it
fn contest(district: ElectoralDistrict) -> Result<ListDataContest, AppError> {
    Ok(
        ListDataContest::new(ContestId::new(district.region_number().to_string())?)
            .with_name(district.title()),
    )
}

/// The number of a set; `None` for a single district
pub fn set_number(set: &ListData) -> Option<NonZeroU64> {
    set.belongs_to_set
        .as_ref()
        .and_then(|number| number.copied_value().ok())
}

/// The region numbers of a set's districts, like "1, 3"
pub fn region_numbers(set: &ListData) -> String {
    set.contests
        .iter()
        .map(|contest| contest.id.raw())
        .collect::<Vec<_>>()
        .join(", ")
}

/// A group's lists that have at least one remaining candidate, one per
/// district, batched into sets of equal lists
#[derive(Debug, Clone)]
pub struct EstablishedLists {
    /// The first created list when the group has several in a district
    lists: Vec<(ElectoralDistrict, CandidateList)>,
    sets: ListSets,
}

impl EstablishedLists {
    /// `None` when no list has a remaining candidate
    pub fn new(store: &CsbStream, scrapped: &Scrapped) -> Result<Option<Self>, AppError> {
        let mut lists = Vec::new();
        for (district, list) in valid_lists_by_district(store, scrapped) {
            if lists.iter().any(|(d, _, _)| *d == district) {
                continue;
            }
            let candidates: Vec<PersonId> = list
                .candidates
                .iter()
                .copied()
                .filter(|person| !scrapped.is_candidate_scrapped(list.id, *person))
                .collect();
            if !candidates.is_empty() {
                lists.push((district, list, candidates));
            }
        }

        Self::from_lists(lists)
    }

    fn from_lists(
        lists: Vec<(ElectoralDistrict, CandidateList, Vec<PersonId>)>,
    ) -> Result<Option<Self>, AppError> {
        let mut batches: Vec<(&[PersonId], Vec<ElectoralDistrict>)> = Vec::new();
        for (district, _, candidates) in &lists {
            match batches
                .iter_mut()
                .find(|(c, _)| *c == candidates.as_slice())
            {
                Some((_, districts)) => districts.push(*district),
                None => batches.push((candidates.as_slice(), vec![*district])),
            }
        }
        let Some(sets) = ListSets::new(batches.into_iter().map(|(_, districts)| districts))? else {
            return Ok(None);
        };

        Ok(Some(Self {
            lists: lists
                .into_iter()
                .map(|(district, list, _)| (district, list))
                .collect(),
            sets,
        }))
    }

    /// The list in `district`, or the first list for a single-district
    /// election (`None`)
    pub fn list(
        &self,
        district: Option<ElectoralDistrict>,
    ) -> Option<(ElectoralDistrict, &CandidateList)> {
        match district {
            Some(district) => self.lists.iter().find(|(d, _)| *d == district),
            None => self.lists.first(),
        }
        .map(|(district, list)| (*district, list))
    }

    pub fn sets(&self) -> &ListSets {
        &self.sets
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ElectoralDistrict::{Bonaire, Drenthe, Fryslan, Groningen, Overijssel, Utrecht};

    /// One list per entry: its district and remaining candidates, where equal
    /// numbers are the same person
    fn established(lists: &[(ElectoralDistrict, &[usize])]) -> Option<EstablishedLists> {
        let persons: Vec<PersonId> = (0..3).map(|_| PersonId::new()).collect();
        EstablishedLists::from_lists(
            lists
                .iter()
                .map(|(district, candidates)| {
                    let candidates = candidates.iter().map(|index| persons[*index]).collect();
                    (*district, CandidateList::default(), candidates)
                })
                .collect(),
        )
        .unwrap()
    }

    /// Per set its region numbers and number
    fn numbered(sets: &ListSets) -> Vec<(String, Option<u64>)> {
        sets.iter()
            .map(|set| (region_numbers(set), set_number(set).map(NonZeroU64::get)))
            .collect()
    }

    fn sets(batches: &[&[ElectoralDistrict]]) -> ListSets {
        ListSets::new(batches.iter().map(|batch| batch.to_vec()))
            .unwrap()
            .unwrap()
    }

    #[test]
    fn no_lists_are_not_established() {
        assert!(established(&[]).is_none());
        assert!(ListSets::new([vec![]]).unwrap().is_none());
    }

    #[test]
    fn lists_with_the_same_candidates_in_the_same_order_are_batched() {
        let established =
            established(&[(Groningen, &[0, 1]), (Utrecht, &[1, 0]), (Drenthe, &[0, 1])]).unwrap();
        assert_eq!(
            numbered(established.sets()),
            [("1, 3".to_string(), Some(1)), ("7".to_string(), None)]
        );
    }

    #[test]
    fn list_is_the_one_in_the_district_or_the_first() {
        let established = established(&[(Utrecht, &[0]), (Groningen, &[1])]).unwrap();
        assert_eq!(established.list(Some(Groningen)).unwrap().0, Groningen);
        assert!(established.list(Some(Drenthe)).is_none());
        assert_eq!(established.list(None).unwrap().0, Utrecht);
    }

    #[test]
    fn a_set_names_its_districts() {
        let sets = sets(&[&[Drenthe, Groningen]]);
        let names: Vec<_> = sets
            .iter()
            .flat_map(|set| set.contests.iter().map(|contest| contest.name.clone()))
            .collect();
        assert_eq!(names, [Some("Groningen".into()), Some("Drenthe".into())]);
    }

    #[test]
    fn a_list_in_one_district_is_standalone_without_a_set_number() {
        let sets = sets(&[&[Utrecht]]);
        assert_eq!(sets.affiliation_type(), AffiliationType::StandAloneList);
        assert_eq!(numbered(&sets), [("7".to_string(), None)]);
    }

    #[test]
    fn one_set_in_several_districts_is_a_set_of_equal_lists() {
        let sets = sets(&[&[Drenthe, Groningen]]);
        assert_eq!(sets.affiliation_type(), AffiliationType::SetOfEqualLists);
        assert_eq!(numbered(&sets), [("1, 3".to_string(), Some(1))]);
    }

    #[test]
    fn several_sets_are_a_group_of_lists() {
        assert_eq!(
            sets(&[&[Groningen], &[Drenthe]]).affiliation_type(),
            AffiliationType::GroupOfLists
        );
        assert_eq!(
            sets(&[&[Groningen, Drenthe], &[Utrecht]]).affiliation_type(),
            AffiliationType::GroupOfLists
        );
    }

    /// A single district neither takes a set number nor moves the next set's
    /// number up
    #[test]
    fn a_single_district_before_a_set_does_not_count() {
        let sets = sets(&[&[Fryslan, Drenthe], &[Groningen], &[Overijssel, Bonaire]]);
        assert_eq!(
            numbered(&sets),
            [
                ("1".to_string(), None),
                ("2, 3".to_string(), Some(1)),
                ("4, 13".to_string(), Some(2)),
            ]
        );
    }

    #[test]
    fn sets_are_numbered_on_their_lowest_district() {
        // Bonaire (13) sorts after Utrecht (7)
        let sets = sets(&[
            &[Bonaire, Utrecht],
            &[Overijssel, Fryslan],
            &[Drenthe, Groningen],
        ]);
        assert_eq!(
            numbered(&sets),
            [
                ("1, 3".to_string(), Some(1)),
                ("2, 4".to_string(), Some(2)),
                ("7, 13".to_string(), Some(3)),
            ]
        );
    }

    #[test]
    fn set_number_is_that_of_the_districts_set() {
        let sets = sets(&[&[Groningen], &[Fryslan, Drenthe]]);
        assert_eq!(sets.set_number(&Groningen), None);
        assert_eq!(sets.set_number(&Drenthe), NonZeroU64::new(1));
        assert_eq!(sets.set_number(&Utrecht), None);
        assert!(sets.contains(&Groningen));
        assert!(!sets.contains(&Utrecht));
    }
}
