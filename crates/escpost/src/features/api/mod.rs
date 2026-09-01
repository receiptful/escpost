//! HTTP adapters for application features.

mod error;
mod origin;
mod print;

pub(crate) fn router() -> axum::Router<crate::web::WebState> {
    use axum::extract::DefaultBodyLimit;
    use axum::middleware;

    print::router()
        .layer(DefaultBodyLimit::max(8 * 1024 * 1024))
        .route_layer(middleware::from_fn(origin::guard))
}
