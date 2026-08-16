use axum::extract::Path;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Redirect, Response};
use rust_embed::RustEmbed;

const NO_CACHE: &str = "no-cache";
const IMMUTABLE: &str = "public, max-age=31536000, immutable";

#[derive(RustEmbed)]
#[folder = "frontend/dist/"]
struct FrontendAssets;

pub(super) async fn redirect() -> Redirect {
    Redirect::permanent("/app/")
}

pub(super) async fn index() -> Response {
    response_for("index.html", NO_CACHE)
}

pub(super) async fn asset(Path(path): Path<String>) -> Response {
    if !valid_relative_path(&path) {
        return StatusCode::NOT_FOUND.into_response();
    }
    response_for(&format!("assets/{path}"), IMMUTABLE)
}

fn valid_relative_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && path
            .split('/')
            .all(|segment| !matches!(segment, "" | "." | ".."))
}

fn response_for(path: &str, cache_control: &'static str) -> Response {
    let Some(asset) = FrontendAssets::get(path) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let content_type = mime_guess::from_path(path).first_or_octet_stream();
    (
        [
            (header::CONTENT_TYPE, content_type.as_ref()),
            (header::CACHE_CONTROL, cache_control),
        ],
        asset.data.into_owned(),
    )
        .into_response()
}
