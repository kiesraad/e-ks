//! Askama template filters for, among others, display, translation, and validation errors.
//! Used to keep the formatting logic out of the templates.
use chrono::{DateTime, Utc};

use crate::{
    ElectionConfig, ElectoralDistrict, Locale,
    constants::{DATE_TIME_SECONDS_FORMAT, DEFAULT_DATE_TIME_FORMAT, DEFAULT_TIMEZONE},
    core::AnyLocale,
    form::FormData,
    structs::persons::Person,
};

// Templates reach the asset URLs through the filters namespace.
pub use super::assets::asset_path;

#[askama::filter_fn]
pub fn display<T: std::fmt::Display>(
    value: &Option<T>,
    _: &dyn askama::Values,
) -> askama::Result<String> {
    Ok(value.as_ref().map(ToString::to_string).unwrap_or_default())
}

/// Format a UTC timestamp in the default timezone with the given format.
fn format_local(value: &DateTime<Utc>, format: &str) -> String {
    value
        .with_timezone(DEFAULT_TIMEZONE)
        .format(format)
        .to_string()
}

#[askama::filter_fn]
pub fn datetime(value: &DateTime<Utc>, _: &dyn askama::Values) -> askama::Result<String> {
    Ok(format_local(value, DEFAULT_DATE_TIME_FORMAT))
}

#[askama::filter_fn]
pub fn datetime_seconds(value: &DateTime<Utc>, _: &dyn askama::Values) -> askama::Result<String> {
    Ok(format_local(value, DATE_TIME_SECONDS_FORMAT))
}

#[askama::filter_fn]
pub fn value_true(value_name: &str, values: &dyn askama::Values) -> askama::Result<bool> {
    let value = askama::get_value::<bool>(values, value_name)?;

    Ok(*value)
}

#[askama::filter_fn]
pub fn locale_value(value_name: &str, values: &dyn askama::Values) -> askama::Result<Locale> {
    let value = askama::get_value::<Locale>(values, value_name)?;

    Ok(*value)
}

#[askama::filter_fn]
pub fn election_value(
    value_name: &str,
    values: &dyn askama::Values,
) -> askama::Result<ElectionConfig> {
    let value = askama::get_value::<ElectionConfig>(values, value_name)?;

    Ok(*value)
}

#[askama::filter_fn]
pub fn integer_value(value_name: &str, values: &dyn askama::Values) -> askama::Result<usize> {
    let value = askama::get_value::<usize>(values, value_name)?;

    Ok(*value)
}

#[askama::filter_fn]
pub fn optional_str_value(
    value_name: &str,
    values: &dyn askama::Values,
) -> askama::Result<Option<&'static str>> {
    let value = askama::get_value::<Option<&'static str>>(values, value_name)?;

    Ok(*value)
}

#[askama::filter_fn]
pub fn str_value(value_name: &str, values: &dyn askama::Values) -> askama::Result<String> {
    let value = askama::get_value::<String>(values, value_name)?;

    Ok(value.clone())
}

#[askama::filter_fn]
pub fn initials_as_printed_on_list(
    value: &Person,
    values: &dyn askama::Values,
) -> askama::Result<String> {
    let locale: &Locale = askama::get_value(values, "locale")?;
    let any_locale = AnyLocale::from(*locale);

    Ok(value.initials_as_printed_on_list(any_locale))
}

#[askama::filter_fn]
pub fn district_name(value: &ElectoralDistrict, _: &dyn askama::Values) -> askama::Result<String> {
    Ok(format!("{}. {}", value.region_number(), value.title()))
}

#[askama::filter_fn]
pub fn election_title(
    value: &ElectionConfig,
    values: &dyn askama::Values,
) -> askama::Result<&'static str> {
    let locale: &Locale = askama::get_value(values, "locale")?;
    let any_locale = AnyLocale::from(*locale);

    Ok(value.title(any_locale))
}

#[askama::filter_fn]
pub fn election_type_title(
    value: &ElectionConfig,
    values: &dyn askama::Values,
) -> askama::Result<&'static str> {
    let locale: &Locale = askama::get_value(values, "locale")?;

    Ok(value.election_type().title(*locale))
}

#[askama::filter_fn]
pub fn flag(country_code: &str, _: &dyn askama::Values) -> askama::Result<String> {
    if !country_code.is_ascii() || country_code.len() != 2 {
        return Ok("🌐".to_string());
    }

    let mut flag = String::new();

    for c in country_code.chars() {
        let code = 0x1f1e6 + c.to_ascii_uppercase() as u32 - 65;

        match char::from_u32(code) {
            Some(flag_char) => flag.push(flag_char),
            None => {
                return Ok("🌐".to_string());
            }
        }
    }

    Ok(flag)
}

#[askama::filter_fn]
pub fn trans(
    key: &str,
    values: &dyn askama::Values,
    #[optional("")] param0: &str,
    #[optional("")] param1: &str,
) -> askama::Result<String> {
    let locale: Locale = *askama::get_value(values, "locale")?;

    if key.is_empty() {
        return Ok("".to_string());
    }

    let mut result = match locale {
        crate::Locale::En => crate::translate::LOCALE_EN.get(key),
        crate::Locale::Nl => crate::translate::LOCALE_NL.get(key),
    }
    .map(ToString::to_string)
    .unwrap_or_else(|| {
        tracing::warn!("Undefined translation key: [{key}]");

        format!("[{key}]")
    });

    if !param0.is_empty() {
        result = result.replacen("{}", param0, 1);

        if !param1.is_empty() {
            result = result.replacen("{}", param1, 1);
        }
    }

    Ok(result)
}

#[askama::filter_fn]
pub fn error<T>(
    form: &FormData<T>,
    values: &dyn askama::Values,
    name: &str,
) -> askama::Result<Vec<String>> {
    let locale: Locale = *askama::get_value(values, "locale")?;

    Ok(form.error(name, locale))
}

#[askama::filter_fn]
pub fn abbreviate_str(s: &str, _: &dyn askama::Values) -> askama::Result<String> {
    Ok(crate::abbreviate_str(s))
}
