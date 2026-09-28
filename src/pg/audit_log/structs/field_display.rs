//! Turn flattened-JSON diff entries into what the audit log shows a user:
//! a translated field label, and a value in the same wording and formatting
//! the rest of the application uses.

use crate::{ElectoralDistrict, Locale, constants::DEFAULT_DATE_FORMAT, trans};

/// Translate a flattened field name to a human-readable label.
///
/// Uses the leaf segment of the dot-notation path (e.g. `name.first_name` -
/// `first_name`) to look up a translation. Array indices become a 1-indexed
/// suffix on the parent field name (e.g. `candidates.3` - `Candidates #4`).
#[expect(
    clippy::cognitive_complexity,
    reason = "A flat translation table; the `trans!` expansions inflate the metric."
)]
pub(super) fn field_label(field: &str, locale: Locale) -> String {
    let leaf = leaf_of(field);
    if let Ok(index) = leaf.parse::<usize>()
        && let Some(parent) = field.rsplit_once('.').map(|(p, _)| p)
    {
        return format!("{} #{}", field_label(parent, locale), index + 1);
    }

    match leaf {
        // Name fields
        "first_name" => trans!("audit_log.detail.fields.first_name", locale),
        "last_name" => trans!("audit_log.detail.fields.last_name", locale),
        "last_name_prefix" => trans!("audit_log.detail.fields.last_name_prefix", locale),
        "initials" => trans!("audit_log.detail.fields.initials", locale),
        // Personal data fields
        "gender" => trans!("audit_log.detail.fields.gender", locale),
        "bsn" => trans!("audit_log.detail.fields.bsn", locale),
        "date_of_birth" => trans!("audit_log.detail.fields.date_of_birth", locale),
        "place_of_residence" => trans!("audit_log.detail.fields.place_of_residence", locale),
        "representative" => trans!("audit_log.detail.fields.representative", locale),
        // Address fields
        "street_name" => trans!("audit_log.detail.fields.street_name", locale),
        "house_number" => trans!("audit_log.detail.fields.house_number", locale),
        "house_number_addition" => {
            trans!("audit_log.detail.fields.house_number_addition", locale)
        }
        "locality" => trans!("audit_log.detail.fields.locality", locale),
        "postal_code" => trans!("audit_log.detail.fields.postal_code", locale),
        "state_or_province" => trans!("audit_log.detail.fields.state_or_province", locale),
        "country" => trans!("audit_log.detail.fields.country", locale),
        "known_in_bag" => trans!("audit_log.detail.fields.known_in_bag", locale),
        // Political group fields
        "long_list_allowed" => trans!("audit_log.detail.fields.long_list_allowed", locale),
        "legal_name" => trans!("audit_log.detail.fields.legal_name", locale),
        "appellation" => trans!("audit_log.detail.fields.appellation", locale),
        "list_designation" => trans!("audit_log.detail.fields.list_designation", locale),
        "previous_election_results" => {
            trans!("audit_log.detail.fields.previous_election_results", locale)
        }
        // Candidate list fields
        "electoral_districts" => trans!("audit_log.detail.fields.electoral_districts", locale),
        "candidates" => trans!("audit_log.detail.fields.candidates", locale),
        // System event fields
        "person_id" => trans!("audit_log.detail.fields.person_id", locale),
        "political_group_id" => trans!("audit_log.detail.fields.political_group_id", locale),
        "stream_id" => trans!("audit_log.detail.fields.stream_id", locale),
        "file_name" => trans!("audit_log.detail.fields.file_name", locale),
        "file_size" => trans!("audit_log.detail.fields.file_size", locale),
        "created_persons" => trans!("audit_log.detail.fields.created_persons", locale),
        "updated_persons" => trans!("audit_log.detail.fields.updated_persons", locale),
        "download_path" => trans!("audit_log.detail.fields.download_path", locale),
        "list_id" => trans!("audit_log.detail.fields.list_id", locale),
        // Fallback: use the raw field name
        _ => field.to_string(),
    }
}

/// Translate a flattened field value to the wording the application shows
/// elsewhere.
/// An empty value means the field was absent on that side of the diff and is
/// left untouched, so that added and removed fields keep rendering as such.
pub(super) fn field_value(field: &str, value: &str, locale: Locale) -> String {
    if value.is_empty() {
        return String::new();
    }

    match (leaf_of(field), value) {
        ("gender", "female") => trans!("common.gender.female", locale),
        ("gender", "male") => trans!("common.gender.male", locale),
        ("bsn", "NoneConfirmed") => trans!("audit_log.detail.values.bsn_none_confirmed", locale),
        ("list_designation", "standalone") => {
            trans!("political_group.type.registered_name", locale)
        }
        ("list_designation", "blank") => trans!("political_group.type.blank_name", locale),
        ("list_designation", "combined") => trans!("political_group.type.name_combination", locale),
        ("previous_election_results", "zero_seats") => {
            trans!("political_group.type.zero_seats", locale)
        }
        ("previous_election_results", "one_to_fifteen_seats") => {
            trans!("political_group.type.one_to_fifteen_seats", locale)
        }
        ("previous_election_results", "sixteen_or_more_seats") => {
            trans!("political_group.type.sixteen_or_more_seats", locale)
        }
        (_, "true") => trans!("audit_log.detail.values.bool_true", locale),
        (_, "false") => trans!("audit_log.detail.values.bool_false", locale),
        ("date_of_birth", date) => format_iso_date(date),
        ("electoral_districts", districts) => districts
            .split(", ")
            .map(district_title)
            .collect::<Vec<_>>()
            .join(", "),
        _ => value.to_string(),
    }
}

fn leaf_of(field: &str) -> &str {
    field.rsplit('.').next().unwrap_or(field)
}

/// The flattened diff carries `ElectoralDistrict`'s serde tag (the bare enum
/// identifier, e.g. `Fryslan`), which drops the diacritics and punctuation
/// `title()` restores (e.g. `Fryslân`, `'s-Gravenhage`). Anything that fails
/// to parse back is passed through unchanged.
fn district_title(tag: &str) -> String {
    serde_json::from_value::<ElectoralDistrict>(serde_json::Value::String(tag.to_string()))
        .map(|district| district.title().to_string())
        .unwrap_or_else(|_| tag.to_string())
}

/// Reformat a serialized ISO date as the day-first format used throughout the
/// application. Anything that does not parse is passed through unchanged.
fn format_iso_date(value: &str) -> String {
    chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d").map_or_else(
        |_| value.to_string(),
        |d| d.format(DEFAULT_DATE_FORMAT).to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const EN: Locale = Locale::En;

    // --- field_label() ---

    #[test]
    fn label_known_fields() {
        assert_eq!(field_label("first_name", EN), "First name");
        assert_eq!(field_label("last_name", EN), "Last name");
        assert_eq!(field_label("gender", EN), "Gender");
        assert_eq!(field_label("postal_code", EN), "Postal code");
    }

    #[test]
    fn label_nested_field_uses_leaf() {
        assert_eq!(field_label("name.first_name", EN), "First name");
        assert_eq!(field_label("personal_data.gender", EN), "Gender");
    }

    #[test]
    fn label_array_index_appends_1_indexed_position() {
        assert_eq!(
            field_label("electoral_districts.0", EN),
            "Electoral districts #1"
        );
        assert_eq!(field_label("candidates.3", EN), "Candidates #4");
    }

    #[test]
    fn label_political_group_fields() {
        assert_eq!(field_label("list_designation", EN), "List type");
        assert_eq!(
            field_label("previous_election_results", EN),
            "Result of the previous election"
        );
    }

    #[test]
    fn label_unknown_field_returns_raw() {
        assert_eq!(field_label("some_unknown_field", EN), "some_unknown_field");
    }

    #[test]
    fn label_dutch_locale() {
        assert_eq!(field_label("first_name", Locale::Nl), "Roepnaam");
    }

    // --- field_value() ---

    #[test]
    fn value_empty_stays_empty() {
        assert_eq!(field_value("personal_data.gender", "", EN), "");
    }

    #[test]
    fn value_gender_is_translated() {
        assert_eq!(field_value("personal_data.gender", "female", EN), "Female");
        assert_eq!(
            field_value("personal_data.gender", "male", Locale::Nl),
            "man"
        );
    }

    #[test]
    fn value_confirmed_absent_bsn_is_translated() {
        assert_eq!(
            field_value("personal_data.bsn", "NoneConfirmed", EN),
            "Confirmed: no social security number"
        );
    }

    #[test]
    fn value_bsn_itself_is_passed_through() {
        assert_eq!(
            field_value("personal_data.bsn", "900194054", EN),
            "900194054"
        );
    }

    #[test]
    fn value_list_designation_is_translated() {
        assert_eq!(
            field_value("list_designation", "standalone", EN),
            "Standalone registered name"
        );
        assert_eq!(field_value("list_designation", "blank", EN), "Blank list");
        assert_eq!(
            field_value("list_designation", "combined", EN),
            "Combination of multiple registered names"
        );
    }

    #[test]
    fn value_previous_election_results_is_translated() {
        assert_eq!(
            field_value("previous_election_results", "zero_seats", EN),
            "0 seats, or did not participate"
        );
        assert_eq!(
            field_value("previous_election_results", "one_to_fifteen_seats", EN),
            "1 to 15 seats"
        );
        assert_eq!(
            field_value("previous_election_results", "sixteen_or_more_seats", EN),
            "16 or more seats"
        );
    }

    #[test]
    fn value_booleans_are_translated() {
        assert_eq!(field_value("address.known_in_bag", "true", EN), "Yes");
        assert_eq!(field_value("address.known_in_bag", "false", EN), "No");
    }

    #[test]
    fn value_date_of_birth_uses_day_first_format() {
        assert_eq!(
            field_value("personal_data.date_of_birth", "1990-02-01", EN),
            "01-02-1990"
        );
    }

    #[test]
    fn value_unparseable_date_is_passed_through() {
        assert_eq!(
            field_value("personal_data.date_of_birth", "not-a-date", EN),
            "not-a-date"
        );
    }

    #[test]
    fn value_electoral_districts_uses_titles_not_serde_tags() {
        use crate::ElectoralDistrict;

        assert_eq!(
            // Zuid-Holland becomes hypenated
            field_value("electoral_districts", "ZuidHolland", EN),
            ElectoralDistrict::ZuidHolland.title()
        );
        assert_eq!(
            field_value("electoral_districts", "Groningen, Fryslan", EN),
            format!(
                "{}, {}",
                ElectoralDistrict::Groningen.title(),
                ElectoralDistrict::Fryslan.title()
            )
        );
    }

    #[test]
    fn value_unknown_field_is_passed_through() {
        assert_eq!(field_value("name.last_name", "Jansen", EN), "Jansen");
    }

    /// Every field the audit log can actually render must have a label; the
    /// raw fallback leaks the serde field path (`personal_data.place_of_residence`)
    /// into the page, which is what issue #1157 reported.
    #[test]
    fn every_serialized_field_has_a_label() {
        use crate::{
            pg::audit_log::structs::{audit_log_detail::is_excluded_field, json_flatten::flatten},
            structs::{
                candidate_lists::CandidateListId, list_submitters::ListSubmitterId,
                name_authorisations::NameAuthorisationId, persons::PersonId,
            },
            test_utils::{
                sample_candidate_list, sample_list_submitter, sample_name_authorisation,
                sample_person, sample_person_from_brp, sample_political_group,
            },
        };

        let mut with_representative = sample_person(PersonId::new());
        with_representative.representative = Some(crate::structs::persons::Representative {
            name: with_representative.name.clone(),
            address: with_representative.address.clone(),
        });

        let entities = [
            serde_json::to_value(sample_person(PersonId::new())).unwrap(),
            serde_json::to_value(sample_person_from_brp()).unwrap(),
            serde_json::to_value(&with_representative).unwrap(),
            serde_json::to_value(sample_candidate_list(CandidateListId::new())).unwrap(),
            serde_json::to_value(sample_political_group()).unwrap(),
            serde_json::to_value(sample_name_authorisation(NameAuthorisationId::new())).unwrap(),
            serde_json::to_value(sample_list_submitter(ListSubmitterId::new())).unwrap(),
            // System events, whose payloads are synthesized rather than stored.
            serde_json::json!({
                "person_id": "", "stream_id": "", "file_name": "", "file_size": 0,
                "download_path": "", "list_id": "", "created_persons": [], "updated_persons": [],
            }),
        ];

        for entity in &entities {
            for key in flatten(entity, "").keys() {
                if is_excluded_field(key) {
                    continue;
                }
                assert_ne!(
                    field_label(key, EN),
                    *key,
                    "field `{key}` has no translation and falls back to its raw name"
                );
            }
        }
    }
}
