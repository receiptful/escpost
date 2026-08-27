use std::net::SocketAddr;

use tokio::net::TcpListener;

use crate::error::CliError;
use crate::web::{self as transport, JobStore};

pub(crate) async fn bind(requested: Option<SocketAddr>) -> Result<TcpListener, CliError> {
    transport::bind(requested)
        .await
        .map_err(|failure| match failure {
            crate::net::BindFailure::Address { address, source } => {
                CliError::BindWeb { address, source }
            }
            crate::net::BindFailure::RangeExhausted => CliError::NoAutomaticWebPort,
        })
}

pub(crate) async fn serve(
    listener: TcpListener,
    jobs: JobStore,
    virtual_printer_address: Option<SocketAddr>,
    open_browser: bool,
    web_app: bool,
) -> Result<(), CliError> {
    let address = listener.local_addr().map_err(CliError::ServeWeb)?;
    let url = format!("http://{address}/");
    for line in viewer_status(address, web_app) {
        eprintln!("{line}");
    }
    if open_browser && let Err(error) = webbrowser::open(&url) {
        eprintln!("warning: could not open the browser ({error}); open {url} manually");
    }

    transport::serve(listener, jobs, virtual_printer_address, web_app)
        .await
        .map_err(CliError::ServeWeb)
}

fn viewer_status(address: SocketAddr, web_app: bool) -> Vec<String> {
    let mut lines = Vec::with_capacity(4);
    if !address.ip().is_loopback() {
        lines.push(format!(
            "warning: receipt data is exposed beyond loopback on {address}"
        ));
    }
    if web_app {
        lines.push(format!("Web app: http://{address}/"));
    }
    lines.push(format!("API: http://{address}/api"));
    lines.push("Press Ctrl+C to stop.".to_owned());
    lines
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

    use super::viewer_status;

    #[test]
    fn loopback_viewer_status_contains_only_cli_guidance() {
        let address = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 9000));

        assert_eq!(
            viewer_status(address, true),
            [
                "Web app: http://127.0.0.1:9000/".to_owned(),
                "API: http://127.0.0.1:9000/api".to_owned(),
                "Press Ctrl+C to stop.".to_owned(),
            ]
        );
    }

    #[test]
    fn status_without_the_web_application_names_the_api_only() {
        let address = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 9000));

        assert_eq!(
            viewer_status(address, false),
            [
                "API: http://127.0.0.1:9000/api".to_owned(),
                "Press Ctrl+C to stop.".to_owned(),
            ]
        );
    }

    #[test]
    fn exposed_viewer_status_adds_the_cli_warning_once() {
        let address = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 9000));

        assert_eq!(
            viewer_status(address, true),
            [
                "warning: receipt data is exposed beyond loopback on 0.0.0.0:9000".to_owned(),
                "Web app: http://0.0.0.0:9000/".to_owned(),
                "API: http://0.0.0.0:9000/api".to_owned(),
                "Press Ctrl+C to stop.".to_owned(),
            ]
        );
    }
}
