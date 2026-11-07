use axum::{
    extract::{FromRequestParts, Query},
    http::request::Parts,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::AppError;

#[derive(Debug, Copy, Clone, Deserialize, Serialize, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SortDirection {
    #[default]
    Asc,
    Desc,
}

impl SortDirection {
    /// Reverse the current sort direction.
    pub fn reverse(&self) -> SortDirection {
        match self {
            SortDirection::Asc => SortDirection::Desc,
            SortDirection::Desc => SortDirection::Asc,
        }
    }
}

/// Sort parameter for paginated views without sortable columns.
#[derive(Debug, Default, Copy, Clone, PartialEq, Serialize, Deserialize)]
pub struct NoSort;

/// Raw pagination query parameters the client can supply.
///
/// `Sort` is the view's sort enum; it defaults to [`NoSort`] for views without
/// sortable columns.
#[derive(Debug, Deserialize, Serialize)]
pub struct Pagination<Sort: Default + PartialEq = NoSort> {
    /// Requested page number (1-indexed). Defaults to `1`.
    #[serde(default = "default_page")]
    #[serde(skip_serializing_if = "is_default_page")]
    pub page: usize,
    /// Requested page size. Defaults to [`default_per_page`].
    #[serde(default = "default_per_page")]
    #[serde(skip_serializing_if = "is_default_per_page")]
    pub per_page: usize,
    /// Optional field to sort by.
    #[serde(default)]
    #[serde(skip_serializing_if = "is_default")]
    pub sort: Sort,
    /// Optional sort order.
    #[serde(default)]
    #[serde(skip_serializing_if = "is_default")]
    pub order: SortDirection,
}

/// Default page when the user omits or zeroes the parameter.
const fn default_page() -> usize {
    1
}

fn is_default_page(page: &usize) -> bool {
    *page == default_page()
}

/// Default page size when unspecified.
const fn default_per_page() -> usize {
    500
}

fn is_default_per_page(per_page: &usize) -> bool {
    *per_page == default_per_page()
}

fn is_default<T: Default + PartialEq>(t: &T) -> bool {
    *t == Default::default()
}

impl<Sort> Default for Pagination<Sort>
where
    Sort: Default + PartialEq,
{
    fn default() -> Self {
        Self {
            page: default_page(),
            per_page: default_per_page(),
            sort: Sort::default(),
            order: SortDirection::default(),
        }
    }
}

impl<S, Sort> FromRequestParts<S> for Pagination<Sort>
where
    S: Send + Sync,
    Sort: DeserializeOwned + Serialize + Default + PartialEq,
    Pagination<Sort>: DeserializeOwned,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let Query(pagination) = Query::<Pagination<Sort>>::from_request_parts(parts, state).await?;

        Ok(pagination)
    }
}

impl<Sort> Pagination<Sort>
where
    Sort: Serialize + Copy + PartialEq + Default,
{
    pub fn as_query(&self) -> String {
        match serde_urlencoded::to_string(self) {
            Ok(query) if !query.is_empty() => format!("?{}", query),
            _ => String::from("?"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Copy, Clone, Deserialize, Serialize, Default, PartialEq)]
    #[serde(rename_all = "snake_case")]
    enum DummySort {
        #[default]
        Name,
        Age,
    }

    #[test]
    fn reverses_sort_direction() {
        assert_eq!(SortDirection::Asc.reverse(), SortDirection::Desc);
        assert_eq!(SortDirection::Desc.reverse(), SortDirection::Asc);
    }

    #[test]
    fn omits_defaults_in_query_string() {
        let pagination: Pagination<DummySort> = Pagination::default();
        assert_eq!(pagination.as_query(), "?");
    }

    #[test]
    fn serializes_all_fields_in_query_string() {
        let pagination = Pagination {
            page: 2,
            per_page: 15,
            sort: DummySort::Age,
            order: SortDirection::Desc,
        };

        assert_eq!(
            pagination.as_query(),
            "?page=2&per_page=15&sort=age&order=desc"
        );
    }
}
