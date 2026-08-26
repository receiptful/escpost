use std::io::{self, Write};
use std::path::PathBuf;

use crossterm::{
    cursor, execute,
    terminal::{Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};

use super::{ConnectionFacts, Printer};
use crate::error::CliError;
use crate::features::printers::monitor::{PrinterMonitor, Snapshot};
use crate::features::printers::{Availability, Transport};

pub(crate) async fn run(
    config: Option<PathBuf>,
    transport: Option<Transport>,
) -> Result<(), CliError> {
    let monitor = PrinterMonitor::new(config);
    let mut subscription = monitor.subscribe();
    let mut terminal = TerminalSession::enter(io::stdout().lock())?;
    terminal.draw_text("Checking configured printers…\n\nPress Ctrl+C to stop.\n")?;

    loop {
        tokio::select! {
            snapshot = subscription.next() => match snapshot {
                Some(snapshot) => terminal.draw_snapshot(&snapshot, transport)?,
                None => return Ok(()),
            },
            signal = tokio::signal::ctrl_c() => {
                signal.map_err(CliError::PrinterMonitorSignal)?;
                return Ok(());
            }
        }
    }
}

struct TerminalSession<W: Write> {
    output: W,
}

impl<W: Write> TerminalSession<W> {
    fn enter(output: W) -> Result<Self, CliError> {
        let mut terminal = Self { output };
        execute!(
            terminal.output,
            EnterAlternateScreen,
            cursor::Hide,
            Clear(ClearType::All),
            cursor::MoveTo(0, 0),
        )
        .map_err(CliError::WriteHumanOutput)?;
        Ok(terminal)
    }

    fn draw_snapshot(
        &mut self,
        snapshot: &Snapshot,
        transport: Option<Transport>,
    ) -> Result<(), CliError> {
        self.draw_text(&render_frame(snapshot, transport))
    }

    fn draw_text(&mut self, text: &str) -> Result<(), CliError> {
        execute!(self.output, cursor::MoveTo(0, 0), Clear(ClearType::All))
            .map_err(CliError::WriteHumanOutput)?;
        self.output
            .write_all(text.as_bytes())
            .and_then(|()| self.output.flush())
            .map_err(CliError::WriteHumanOutput)
    }
}

impl<W: Write> Drop for TerminalSession<W> {
    fn drop(&mut self) {
        let _ = execute!(self.output, cursor::Show, LeaveAlternateScreen);
    }
}

fn render_frame(snapshot: &Snapshot, transport: Option<Transport>) -> String {
    let updated_at = snapshot.updated_at.time();
    let mut frame = format!(
        "Known printers — updated {:02}:{:02}:{:02}\n",
        updated_at.hour(),
        updated_at.minute(),
        updated_at.second(),
    );

    if let Some(warning) = &snapshot.warning {
        frame.push_str(&format!("Warning: {warning}\n"));
    }

    let printers = snapshot
        .printers
        .iter()
        .filter(|printer| transport.is_none_or(|transport| printer.transport == transport))
        .collect::<Vec<_>>();
    if printers.is_empty() {
        if snapshot.printers.is_empty() {
            frame.push_str("\nNo printers configured.\n");
        } else {
            frame.push_str("\nNo printers match the transport filter.\n");
        }
    } else {
        frame.push_str("\nNAME                 STATUS       CONNECTION\n");
        for printer in printers {
            frame.push_str(&format_printer_row(printer));
        }
    }
    frame.push_str("\nPress Ctrl+C to stop.\n");
    frame
}

fn format_printer_row(printer: &Printer) -> String {
    let status = match printer.availability {
        Availability::Connected => "connected",
        Availability::Unavailable => "unavailable",
    };
    format!(
        "{:<20} {:<12} {}\n",
        printer.name,
        status,
        connection_summary(&printer.connection),
    )
}

fn connection_summary(connection: &ConnectionFacts) -> String {
    match connection {
        ConnectionFacts::Usb(usb) => format!("{:04x}:{:04x}", usb.vendor_id, usb.product_id),
        ConnectionFacts::Network(network) => {
            super::super::cli::output::format_network_endpoint(&network.host, network.port)
        }
    }
}

#[cfg(test)]
mod tests {
    use time::OffsetDateTime;

    use super::*;
    use crate::features::printers::list::{
        ConnectionFacts, NetworkConnectionFacts, Printer, UsbConnectionFacts,
    };
    use crate::features::printers::monitor::Snapshot;
    use crate::features::printers::{Availability, Transport};

    #[test]
    fn frame_summarizes_network_and_connected_usb_printers() {
        let frame = render_frame(&snapshot(), None);

        assert!(frame.contains("Known printers — updated 14:32:10"));
        assert!(frame.contains("NAME"));
        assert!(frame.contains("STATUS"));
        assert!(frame.contains("CONNECTION"));
        assert!(frame.contains("kitchen"));
        assert!(frame.contains("connected"));
        assert!(frame.contains("192.168.1.40:9100"));
        assert!(frame.contains("counter"));
        assert!(frame.contains("0416:5011"));
    }

    #[test]
    fn frame_summarizes_an_unavailable_usb_printer() {
        let frame = render_frame(
            &Snapshot {
                printers: vec![usb_printer("counter", Availability::Unavailable)],
                ..snapshot()
            },
            None,
        );

        assert!(frame.contains("counter"));
        assert!(frame.contains("unavailable"));
        assert!(frame.contains("0416:5011"));
    }

    #[test]
    fn frame_includes_inventory_warnings() {
        let frame = render_frame(
            &Snapshot {
                warning: Some("configuration is invalid".to_owned()),
                ..snapshot()
            },
            None,
        );

        assert!(frame.contains("Warning: configuration is invalid"));
    }

    #[test]
    fn frame_explains_when_no_printers_are_configured() {
        let frame = render_frame(
            &Snapshot {
                printers: Vec::new(),
                ..snapshot()
            },
            None,
        );

        assert!(frame.contains("No printers configured."));
    }

    #[test]
    fn frame_filters_displayed_rows_by_transport() {
        let frame = render_frame(&snapshot(), Some(Transport::Network));

        assert!(frame.contains("kitchen"));
        assert!(frame.contains("192.168.1.40:9100"));
        assert!(!frame.contains("counter"));
    }

    fn snapshot() -> Snapshot {
        Snapshot {
            updated_at: OffsetDateTime::from_unix_timestamp(52_330)
                .expect("the fixed timestamp should be valid"),
            warning: None,
            printers: vec![
                Printer {
                    name: "kitchen".to_owned(),
                    transport: Transport::Network,
                    availability: Availability::Connected,
                    profile: Some("REFERENCE".to_owned()),
                    connection: ConnectionFacts::Network(NetworkConnectionFacts {
                        host: "192.168.1.40".to_owned(),
                        port: 9100,
                    }),
                },
                usb_printer("counter", Availability::Connected),
            ],
        }
    }

    fn usb_printer(name: &str, availability: Availability) -> Printer {
        Printer {
            name: name.to_owned(),
            transport: Transport::Usb,
            availability,
            profile: None,
            connection: ConnectionFacts::Usb(UsbConnectionFacts {
                vendor_id: 0x0416,
                product_id: 0x5011,
                bus: None,
                address: None,
                manufacturer: None,
                product: None,
                serial_number: None,
                interface_number: 0,
                out_endpoints: vec![1],
                in_endpoints: Vec::new(),
            }),
        }
    }
}
