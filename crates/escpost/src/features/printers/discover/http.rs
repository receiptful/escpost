//! HTTP adapter for `printers discover`.
//!
//! The browser's scan-options panel needs to know which networks it can scan
//! before any scan starts, including adapters skipped for being larger than
//! the automatic sweep will cover, so a user can add those as a custom
//! subnet instead of wondering why nothing appeared.

use axum::http::header;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;

use crate::discovery::{self, SkipReason};
use crate::web::WebState;
use crate::web::error::ApiError;

pub(crate) fn router() -> Router<WebState> {
    Router::new().route(
        "/api/printers/discover/networks",
        get(networks).fallback(crate::web::error::method_not_allowed),
    )
}

#[derive(Serialize)]
struct NetworksResponse {
    networks: Vec<NetworkResponse>,
    skipped: Vec<SkippedResponse>,
    default_port: u16,
    default_timeout_ms: u64,
}

#[derive(Serialize)]
struct NetworkResponse {
    subnet: String,
    interface: Option<String>,
    hosts: u64,
}

#[derive(Serialize)]
struct SkippedResponse {
    interface: String,
    subnet: Option<String>,
    reason: &'static str,
}

async fn networks() -> Result<
    (
        [(axum::http::HeaderName, &'static str); 1],
        Json<NetworksResponse>,
    ),
    ApiError,
> {
    let addresses = discovery::local_interface_addresses()
        .map_err(|_| ApiError::network_detection_failure())?;
    let (targets, skipped) = discovery::detect_networks(addresses);

    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(NetworksResponse {
            networks: targets
                .iter()
                .map(|target| NetworkResponse {
                    subnet: target.subnet.to_string(),
                    interface: target.interface.clone(),
                    hosts: discovery::probe_count(std::slice::from_ref(target)),
                })
                .collect(),
            skipped: skipped
                .into_iter()
                .map(|adapter| SkippedResponse {
                    interface: adapter.name,
                    subnet: adapter.subnet.map(|subnet| subnet.to_string()),
                    reason: match adapter.reason {
                        SkipReason::TooLarge => "too_large",
                        SkipReason::UnusableNetmask => "unusable_netmask",
                    },
                })
                .collect(),
            default_port: 9100,
            default_timeout_ms: 1000,
        }),
    ))
}
