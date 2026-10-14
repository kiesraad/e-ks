use axum::{extract::State, response::IntoResponse};

use crate::{
    AppError, AppRequestState, CsbContext, CsbMainStore,
    csb::finalise::{
        pages::combined_zip::{
            candidate_list_overview::candidate_list_overview_model,
            candidate_list_per_district::candidate_list_per_district_model,
            candidate_list_publication::candidate_list_publication_model,
        },
        paths::CsbFinalZipDownloadPath,
    },
};

mod candidate_list_overview;
mod candidate_list_per_district;
mod candidate_list_publication;
mod eml_zip;

pub async fn download_final_zip<S: AppRequestState>(
    _: CsbFinalZipDownloadPath,
    context: CsbContext,
    main_store: CsbMainStore,
    State(state): State<S>,
) -> Result<impl IntoResponse, AppError> {
    let candidate_list_publication =
        candidate_list_publication_model(&context, &main_store, &state).await?;
    let candidate_list_per_district =
        candidate_list_per_district_model(&context, &main_store, &state).await?;
    let candidate_list_overview =
        candidate_list_overview_model(&context, &main_store, &state).await?;

    Ok("Handler is bereikbaar")
}
