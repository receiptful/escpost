use axum::extract::rejection::QueryRejection;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderName, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use super::error::ApiError;
use super::{CommandResponse, JobStoreState, WebState};

pub(super) fn router() -> Router<WebState> {
    Router::new()
        .route(
            "/api/jobs/current",
            get(current).fallback(super::error::method_not_allowed),
        )
        .route(
            "/api/jobs/{job_id}/sheets/{sheet_number}",
            get(sheet).fallback(super::error::method_not_allowed),
        )
        .route(
            "/api/jobs/{job_id}/input",
            get(input).fallback(super::error::method_not_allowed),
        )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NoQuery {}

#[derive(Serialize)]
struct CurrentJobResponse {
    receiving: bool,
    profile: String,
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    hint: Option<String>,
    job: Option<JobResponse>,
}

#[derive(Serialize)]
struct JobResponse {
    id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    completed_at_unix_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    completion: Option<&'static str>,
    antialias: bool,
    warnings: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    input_url: Option<String>,
    sheets: Vec<JobSheetResponse>,
}

#[derive(Serialize)]
struct JobSheetResponse {
    number: usize,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    width_dots: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    height_dots: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    image_url: Option<String>,
    commands: Vec<CommandResponse>,
}

async fn current(
    State(state): State<WebState>,
    query: Result<Query<NoQuery>, QueryRejection>,
) -> Result<([(HeaderName, &'static str); 1], Json<CurrentJobResponse>), ApiError> {
    query.map_err(|_| ApiError::invalid_query())?;
    let store = state.jobs.state.read().await;
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(current_response(&store)),
    ))
}

fn current_response(state: &JobStoreState) -> CurrentJobResponse {
    let receiving = state.receiving > 0;
    let job = state.jobs.front().map(|job| {
        let id = state.generation.to_string();
        let sheets = job
            .trace_sheets
            .iter()
            .enumerate()
            .map(|(index, trace_sheet)| {
                let rendered = job.sheets.get(index);
                JobSheetResponse {
                    number: index + 1,
                    name: rendered
                        .map(|sheet| sheet.name.clone())
                        .unwrap_or_else(|| format!("sheet-{:03}", index + 1)),
                    width_dots: rendered.map(|sheet| sheet.width_dots),
                    height_dots: rendered.map(|sheet| sheet.height_dots),
                    image_url: rendered.map(|_| format!("/api/jobs/{id}/sheets/{}", index + 1)),
                    commands: trace_sheet.commands.clone(),
                }
            })
            .collect();
        JobResponse {
            id: id.clone(),
            completed_at_unix_ms: state.completed_at,
            completion: state.completion,
            antialias: state.antialias,
            warnings: job.warnings.clone(),
            input_url: (state.raw_input.is_some() && !receiving)
                .then(|| format!("/api/jobs/{id}/input")),
            sheets,
        }
    });
    CurrentJobResponse {
        receiving,
        profile: state
            .jobs
            .front()
            .map(|job| job.profile.clone())
            .unwrap_or_else(|| state.session_profile.clone()),
        error: state.error.clone(),
        hint: if job.is_none() {
            state.waiting_hint.clone()
        } else {
            None
        },
        job,
    }
}

async fn sheet(
    Path((job_id, sheet_number)): Path<(u64, usize)>,
    State(state): State<WebState>,
    query: Result<Query<NoQuery>, QueryRejection>,
) -> Result<Response, ApiError> {
    query.map_err(|_| ApiError::invalid_query())?;
    let store = state.jobs.state.read().await;
    let job = current_job(&store, job_id)?;
    let sheet = sheet_number
        .checked_sub(1)
        .and_then(|index| job.sheets.get(index))
        .ok_or_else(ApiError::job_not_found)?;
    Ok((
        [
            (header::CONTENT_TYPE, "image/png"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        sheet.png.clone(),
    )
        .into_response())
}

async fn input(
    Path(job_id): Path<u64>,
    State(state): State<WebState>,
    query: Result<Query<NoQuery>, QueryRejection>,
) -> Result<Response, ApiError> {
    query.map_err(|_| ApiError::invalid_query())?;
    let store = state.jobs.state.read().await;
    current_job(&store, job_id)?;
    let bytes = store
        .raw_input
        .clone()
        .ok_or_else(ApiError::job_not_found)?;
    let completed_at = store.completed_at.unwrap_or(0);
    Ok((
        [
            (
                header::CONTENT_TYPE,
                String::from("application/octet-stream"),
            ),
            (header::CACHE_CONTROL, String::from("no-store")),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"escpost-job-{completed_at}.bin\""),
            ),
        ],
        bytes,
    )
        .into_response())
}

fn current_job(
    state: &JobStoreState,
    job_id: u64,
) -> Result<&std::sync::Arc<super::RenderedJob>, ApiError> {
    if state.generation != job_id {
        return Err(ApiError::job_not_found());
    }
    state.jobs.front().ok_or_else(ApiError::job_not_found)
}
