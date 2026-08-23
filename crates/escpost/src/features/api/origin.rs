use axum::extract::{Request, State};
use axum::http::header;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use super::ApiState;
use super::error::ApiFailure;

const EXTENSION_SCHEME: &str = "chrome-extension://";

/// D2. Accept the extension, a local process, or nothing; reject every other
/// origin.
///
/// This is a negative filter and cannot be anything more: Chrome sends no
/// `Origin` at all on an extension's GET requests, so an absent header has to
/// be accepted, which means no request can be *proved* to come from the
/// extension. What the filter does buy is that a remote page cannot reach this
/// port even if Chrome's own local-network policy is one day relaxed.
pub(super) fn origin_allowed(origin: Option<&str>, pinned_extension_id: Option<&str>) -> bool {
    // Absent entirely: curl, a local backend, or an extension GET (L3).
    let Some(origin) = origin else {
        return true;
    };
    // A local file or an opaque origin.
    if origin == "null" {
        return true;
    }
    let Some(id) = origin.strip_prefix(EXTENSION_SCHEME) else {
        return false;
    };
    // An origin has no path; a slash here means this is not one.
    if id.is_empty() || id.contains('/') {
        return false;
    }
    match pinned_extension_id {
        Some(expected) => id == expected,
        // Deliberately not validating the id's shape beyond this. A reinstall
        // changes the id, and the spike's stub pinned the first id it saw and
        // then locked out a rebuilt extension with no recovery. Being strict
        // here would buy no security — the header is unauthenticated either
        // way — while adding exactly that failure mode back.
        None => true,
    }
}

pub(super) async fn guard(State(state): State<ApiState>, request: Request, next: Next) -> Response {
    let origin = request
        .headers()
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok());
    if origin_allowed(origin, state.extension_id.as_deref()) {
        return next.run(request).await;
    }
    ApiFailure::origin_not_granted(origin.unwrap_or_default()).into_response()
}

#[cfg(test)]
mod tests {
    use super::origin_allowed;

    #[test]
    fn an_absent_origin_is_accepted_because_local_processes_send_none() {
        // L3: curl, a Node backend, a cron job. Chrome also sends no Origin on
        // an extension's GET requests, so rejecting this would break the
        // extension's own printer list.
        assert!(origin_allowed(None, None));
    }

    #[test]
    fn a_null_origin_is_accepted() {
        assert!(origin_allowed(Some("null"), None));
    }

    #[test]
    fn any_extension_origin_is_accepted_when_none_is_pinned() {
        assert!(origin_allowed(
            Some("chrome-extension://cnifebiebidolpmlmgcghpopggfcklmc"),
            None
        ));
        assert!(origin_allowed(
            Some("chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
            None
        ));
    }

    #[test]
    fn a_web_page_is_rejected() {
        for origin in [
            "https://evil.example",
            "http://localhost:3000",
            "https://127.0.0.1",
            "file://",
        ] {
            assert!(
                !origin_allowed(Some(origin), None),
                "{origin} should be rejected"
            );
        }
    }

    #[test]
    fn a_web_origin_that_merely_mentions_the_scheme_is_rejected() {
        assert!(!origin_allowed(
            Some("https://evil.example/chrome-extension://x"),
            None
        ));
    }

    #[test]
    fn an_extension_origin_with_no_id_is_rejected() {
        assert!(!origin_allowed(Some("chrome-extension://"), None));
        assert!(!origin_allowed(Some("chrome-extension://a/b"), None));
    }

    #[test]
    fn pinning_an_id_narrows_to_exactly_that_extension() {
        let pinned = Some("cnifebiebidolpmlmgcghpopggfcklmc");
        assert!(origin_allowed(
            Some("chrome-extension://cnifebiebidolpmlmgcghpopggfcklmc"),
            pinned
        ));
        assert!(!origin_allowed(
            Some("chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
            pinned
        ));
    }

    #[test]
    fn pinning_still_accepts_local_processes() {
        // Pinning narrows which extension may call, not whether a local
        // backend may. L1–L4 do not depend on an extension being installed.
        let pinned = Some("cnifebiebidolpmlmgcghpopggfcklmc");
        assert!(origin_allowed(None, pinned));
        assert!(origin_allowed(Some("null"), pinned));
    }
}
