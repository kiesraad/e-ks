use super::PaperCorrected;
use crate::{
    Locale,
    structs::{
        common::{BsnOrNoneConfirmed, DateOfBirth},
        persons::{Person, Representative},
    },
};

/// The personal details of a candidate diffed against the corrections.
pub struct PaperCorrectedPersonDetails {
    pub initials: PaperCorrected,
    pub first_name: PaperCorrected,
    pub last_name_prefix: PaperCorrected,
    pub last_name: PaperCorrected,
    pub gender: PaperCorrected,
    pub date_of_birth: PaperCorrected,
    pub bsn: PaperCorrected,
    pub place_of_residence: PaperCorrected,
    pub street_name: PaperCorrected,
    pub house_number: PaperCorrected,
    pub house_number_addition: PaperCorrected,
    pub postal_code: PaperCorrected,
    pub locality: PaperCorrected,
    pub country: PaperCorrected,
    /// Whether either projection has a representative; the representative
    /// table is hidden when neither does.
    pub has_representative: bool,
    pub representative_name: PaperCorrected,
    pub representative_street_name: PaperCorrected,
    pub representative_house_number: PaperCorrected,
    pub representative_house_number_addition: PaperCorrected,
    pub representative_postal_code: PaperCorrected,
    pub representative_locality: PaperCorrected,
}

impl PaperCorrectedPersonDetails {
    pub fn new(
        imported: Option<&Person>,
        corrected: Option<&Person>,
        csb_corrected: Option<&Person>,
        locale: Locale,
    ) -> Self {
        // Diff one field of both projections; `csb_field` includes the CSB
        // correction of the same field.
        let field = |f: fn(&Person) -> String| PaperCorrected::from_field(imported, corrected, f);
        let csb_field =
            |f: fn(&Person) -> String| field(f).with_csb_correction(csb_corrected.map(f));

        Self {
            initials: csb_field(|p| p.name.initials.to_string()),
            first_name: field(|p| opt_display(&p.name.first_name)),
            last_name_prefix: csb_field(|p| opt_display(&p.name.last_name_prefix)),
            last_name: csb_field(|p| p.name.last_name.to_string()),
            gender: PaperCorrected::from_field(imported, corrected, |p| p.gender_label(locale)),
            date_of_birth: csb_field(|p| {
                DateOfBirth::format_option(&p.personal_data.date_of_birth)
            }),
            bsn: field(|p| {
                p.personal_data
                    .bsn
                    .as_ref()
                    .map(BsnOrNoneConfirmed::to_exposed_string)
                    .unwrap_or_default()
            }),
            place_of_residence: csb_field(|p| opt_display(&p.personal_data.place_of_residence)),
            street_name: field(|p| opt_display(&p.address.street_name)),
            house_number: field(|p| opt_display(&p.address.house_number)),
            house_number_addition: field(|p| opt_display(&p.address.house_number_addition)),
            postal_code: field(|p| opt_display(&p.address.postal_code)),
            locality: field(|p| opt_display(&p.address.locality)),
            country: field(|p| opt_display(&p.personal_data.country)),
            has_representative: imported.is_some_and(|p| p.representative.is_some())
                || corrected.is_some_and(|p| p.representative.is_some()),
            representative_name: representative_field(imported, corrected, |r| r.name.display()),
            representative_street_name: representative_field(imported, corrected, |r| {
                opt_display(&r.address.street_name)
            }),
            representative_house_number: representative_field(imported, corrected, |r| {
                opt_display(&r.address.house_number)
            }),
            representative_house_number_addition: representative_field(imported, corrected, |r| {
                opt_display(&r.address.house_number_addition)
            }),
            representative_postal_code: representative_field(imported, corrected, |r| {
                opt_display(&r.address.postal_code)
            }),
            representative_locality: representative_field(imported, corrected, |r| {
                opt_display(&r.address.locality)
            }),
        }
    }
}

/// Diff one field of the (optional) representative of both projections.
fn representative_field(
    imported: Option<&Person>,
    corrected: Option<&Person>,
    field: impl Fn(&Representative) -> String,
) -> PaperCorrected {
    PaperCorrected::from_field(imported, corrected, |p| {
        p.representative.as_ref().map(&field).unwrap_or_default()
    })
}

fn opt_display<T: std::fmt::Display>(value: &Option<T>) -> String {
    value.as_ref().map(ToString::to_string).unwrap_or_default()
}
