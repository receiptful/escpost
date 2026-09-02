use axum::Extension;
use axum::extract::Request;
use axum::http::header;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::web::WebState;

use super::error::ApiError;

const EXTENSION_SCHEMES: [&str; 3] = [
    "chrome-extension://",
    "moz-extension://",
    "safari-web-extension://",
];

pub(super) fn origin_allowed(origin: Option<&str>, pinned_extension_id: Option<&str>) -> bool {
    let Some(origin) = origin else {
        return true;
    };
    let Some(id) = EXTENSION_SCHEMES
        .iter()
        .find_map(|scheme| origin.strip_prefix(scheme))
    else {
        return false;
    };
    if id.is_empty()
        || id
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || matches!(byte, b'/' | b':' | b'?' | b'#'))
    {
        return false;
    }
    pinned_extension_id.is_none_or(|expected| id == expected)
}

pub(crate) async fn guard(
    Extension(state): Extension<WebState>,
    request: Request,
    next: Next,
) -> Response {
    let allowed = match request.headers().get(header::ORIGIN) {
        None => true,
        Some(origin) => origin
            .to_str()
            .ok()
            .is_some_and(|origin| origin_allowed(Some(origin), state.extension_id.as_deref())),
    };
    if allowed {
        next.run(request).await
    } else {
        ApiError::origin_not_granted().into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::origin_allowed;

    #[test]
    fn absent_and_exact_extension_origins_are_allowed() {
        assert!(origin_allowed(None, None));
        assert!(origin_allowed(Some("chrome-extension://abc"), None));
        assert!(origin_allowed(Some("moz-extension://abc"), None));
        assert!(origin_allowed(Some("safari-web-extension://abc"), None));
    }

    #[test]
    fn opaque_web_and_lookalike_origins_are_rejected() {
        for origin in [
            "null",
            "https://example.invalid",
            "file://",
            "web-extension://abc",
            "chrome-extension://",
            "chrome-extension://abc/path",
            "chrome-extension://abc?query",
        ] {
            assert!(!origin_allowed(Some(origin), None), "accepted {origin}");
        }
    }

    #[test]
    fn a_pin_requires_an_exact_id_but_still_allows_absent_origins() {
        assert!(origin_allowed(None, Some("expected")));
        assert!(origin_allowed(
            Some("chrome-extension://expected"),
            Some("expected")
        ));
        assert!(!origin_allowed(
            Some("chrome-extension://expected-more"),
            Some("expected")
        ));
    }
}
