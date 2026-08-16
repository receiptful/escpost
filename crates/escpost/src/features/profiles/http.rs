use axum::extract::{Query, State};
use axum::http::header;
use axum::routing::get;
use axum::{Json, Router};
use escpost_profiles::ProfileSource;
use serde::Serialize;

use crate::web::WebState;
use crate::web::error::ApiError;

use super::{ListRequest, ProfileFacts, list};

pub(crate) fn router() -> Router<WebState> {
    Router::new().route(
        "/api/profiles/list",
        get(list_profiles).fallback(crate::web::error::method_not_allowed),
    )
}

async fn list_profiles(
    State(_state): State<WebState>,
    query: Result<Query<NoQuery>, axum::extract::rejection::QueryRejection>,
) -> Result<
    (
        [(axum::http::HeaderName, &'static str); 1],
        Json<ListResponse>,
    ),
    ApiError,
> {
    query.map_err(|_| ApiError::invalid_query())?;
    let response = list(ListRequest {
        vendor: None,
        source: None,
        search: None,
    })
    .map_err(|_| ApiError::profile_catalog_failure())?;

    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(ListResponse {
            profiles: response
                .profiles
                .iter()
                .map(ProfileResponse::from)
                .collect(),
        }),
    ))
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct NoQuery {}

#[derive(Serialize)]
struct ListResponse {
    profiles: Vec<ProfileResponse>,
}

#[derive(Serialize)]
struct ProfileResponse {
    id: String,
    vendor: String,
    model: String,
    source: &'static str,
    paper_width_mm: f64,
    printable_width_mm: f64,
    printable_width_dots: u32,
    dpi_x: u32,
    dpi_y: u32,
    full_cut: bool,
    partial_cut: bool,
    barcode_function_a: bool,
    barcode_function_b: bool,
    qr_code: bool,
}

impl From<&ProfileFacts> for ProfileResponse {
    fn from(profile: &ProfileFacts) -> Self {
        Self {
            id: profile.id.clone(),
            vendor: profile.vendor.clone(),
            model: profile.model.clone(),
            source: source_label(&profile.source),
            paper_width_mm: profile.paper_width_mm,
            printable_width_mm: profile.printable_width_mm,
            printable_width_dots: profile.printable_width_dots,
            dpi_x: profile.dpi_x,
            dpi_y: profile.dpi_y,
            full_cut: profile.features.paper_full_cut,
            partial_cut: profile.features.paper_part_cut,
            barcode_function_a: !profile.features.barcodes.function_a.is_empty(),
            barcode_function_b: !profile.features.barcodes.function_b.is_empty(),
            qr_code: profile.features.qr_code,
        }
    }
}

fn source_label(source: &ProfileSource) -> &'static str {
    match source {
        ProfileSource::Upstream => "calibrated",
        ProfileSource::UpstreamDefault => "synthesized",
        ProfileSource::Reference => "virtual",
    }
}
