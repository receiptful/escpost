use std::net::SocketAddr;
use std::time::Duration;

use tokio::net::TcpListener;

use crate::error::CliError;
use crate::web::{self as transport, JobStore, WebConfiguration};

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
    idle_timeout: Option<Duration>,
    open_browser: bool,
    web_app: bool,
    configuration: WebConfiguration,
) -> Result<(), CliError> {
    let address = listener.local_addr().map_err(CliError::ServeWeb)?;
    let url = format!("http://{address}/");
    let virtual_printer = virtual_printer_address.map(|address| (address, idle_timeout));
    for line in listener_status(Some((address, web_app)), virtual_printer) {
        eprintln!("{line}");
    }
    if open_browser && let Err(error) = webbrowser::open(&url) {
        eprintln!("warning: could not open the browser ({error}); open {url} manually");
    }

    transport::serve(
        listener,
        jobs,
        virtual_printer_address,
        web_app,
        configuration,
    )
    .await
    .map_err(CliError::ServeWeb)
}

pub(crate) fn listener_status(
    web: Option<(SocketAddr, bool)>,
    virtual_printer: Option<(SocketAddr, Option<Duration>)>,
) -> Vec<String> {
    let mut lines = Vec::with_capacity(5);
    if let Some((address, _)) = web
        && !address.ip().is_loopback()
    {
        lines.push(format!(
            "warning: receipt data is exposed beyond loopback on {address}"
        ));
    }
    let mut addresses = Vec::with_capacity(3);
    if let Some((address, idle_timeout)) = virtual_printer {
        let idle_timeout = idle_timeout
            .map(|timeout| format!("{timeout:?}"))
            .unwrap_or_else(|| "disabled".to_owned());
        addresses.push((
            "Virtual IP printer:",
            format!("{address} (Idle timeout: {idle_timeout})"),
        ));
    }
    if let Some((address, web_app)) = web {
        if web_app {
            addresses.push(("Web app:", format!("http://{address}/")));
        }
        addresses.push(("API:", format!("http://{address}/api")));
    }
    lines.extend(aligned_rows(addresses));
    if let Some((address, _)) = virtual_printer {
        lines.push("To render a job through the virtual printer, run:".to_owned());
        lines.push(format!("`escpost print file.hex --network {address}`"));
    }
    lines.push("Press Ctrl+C to stop.".to_owned());
    lines
}

fn aligned_rows(rows: Vec<(&str, String)>) -> impl Iterator<Item = String> {
    let width = rows.iter().map(|(label, _)| label.len()).max().unwrap_or(0);
    rows.into_iter()
        .map(move |(label, value)| format!("{label:<width$} {value}"))
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
    use std::time::Duration;

    use super::listener_status;

    #[test]
    fn loopback_viewer_status_contains_only_cli_guidance() {
        let address = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 9000));

        assert_eq!(
            listener_status(Some((address, true)), None),
            [
                "Web app: http://127.0.0.1:9000/".to_owned(),
                "API:     http://127.0.0.1:9000/api".to_owned(),
                "Press Ctrl+C to stop.".to_owned(),
            ]
        );
    }

    #[test]
    fn status_without_the_web_application_names_the_api_only() {
        let address = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 9000));

        assert_eq!(
            listener_status(Some((address, false)), None),
            [
                "API: http://127.0.0.1:9000/api".to_owned(),
                "Press Ctrl+C to stop.".to_owned(),
            ]
        );
    }

    #[test]
    fn raw_only_status_appends_the_disabled_timeout_without_table_padding() {
        let address = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 9100));

        assert_eq!(
            listener_status(None, Some((address, None))),
            [
                "Virtual IP printer: 127.0.0.1:9100 (Idle timeout: disabled)".to_owned(),
                "To render a job through the virtual printer, run:".to_owned(),
                "`escpost print file.hex --network 127.0.0.1:9100`".to_owned(),
                "Press Ctrl+C to stop.".to_owned(),
            ]
        );
    }

    #[test]
    fn combined_status_aligns_only_addresses_and_not_the_print_guidance() {
        let web_address = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 9000));
        let printer_address = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 9100));

        assert_eq!(
            listener_status(
                Some((web_address, true)),
                Some((printer_address, Some(Duration::from_secs(20)))),
            ),
            [
                "Virtual IP printer: 127.0.0.1:9100 (Idle timeout: 20s)".to_owned(),
                "Web app:            http://127.0.0.1:9000/".to_owned(),
                "API:                http://127.0.0.1:9000/api".to_owned(),
                "To render a job through the virtual printer, run:".to_owned(),
                "`escpost print file.hex --network 127.0.0.1:9100`".to_owned(),
                "Press Ctrl+C to stop.".to_owned(),
            ]
        );
    }

    #[test]
    fn exposed_viewer_status_adds_the_cli_warning_once() {
        let address = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 9000));

        assert_eq!(
            listener_status(Some((address, true)), None),
            [
                "warning: receipt data is exposed beyond loopback on 0.0.0.0:9000".to_owned(),
                "Web app: http://0.0.0.0:9000/".to_owned(),
                "API:     http://0.0.0.0:9000/api".to_owned(),
                "Press Ctrl+C to stop.".to_owned(),
            ]
        );
    }
}
