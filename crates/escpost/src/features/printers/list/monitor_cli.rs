use std::io::{self, Write};
use std::path::PathBuf;

use crossterm::{
    cursor, execute,
    terminal::{Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
use unicode_width::UnicodeWidthChar;

use super::{ConnectionFacts, Printer};
use crate::error::CliError;
use crate::features::printers::monitor::{PrinterMonitor, Snapshot};
use crate::features::printers::{Availability, Transport};

const NAME_WIDTH: usize = 20;
const TRANSPORT_WIDTH: usize = 10;
const PROFILE_WIDTH: usize = 16;
const STATUS_WIDTH: usize = 12;
const CONNECTION_WIDTH: usize = 30;

pub(crate) async fn run(
    config: Option<PathBuf>,
    transport: Option<Transport>,
) -> Result<(), CliError> {
    let monitor = PrinterMonitor::new(config);
    let mut subscription = monitor.subscribe();
    let mut terminal = TerminalSession::enter(CrosstermTerminal::new(io::stdout().lock()))?;
    terminal.draw_text("Checking configured printers…\n\nPress Ctrl+C to stop.\n")?;

    loop {
        tokio::select! {
            snapshot = subscription.next() => match snapshot {
                Some(snapshot) => terminal.draw_snapshot(&snapshot, transport)?,
                None => return Ok(()),
            },
            signal = tokio::signal::ctrl_c() => {
                return terminal.finish_signal(signal);
            }
        }
    }
}

trait TerminalCommands {
    fn enter_alternate_screen(&mut self) -> io::Result<()>;
    fn hide_cursor(&mut self) -> io::Result<()>;
    fn clear(&mut self) -> io::Result<()>;
    fn move_home(&mut self) -> io::Result<()>;
    fn write_text(&mut self, text: &str) -> io::Result<()>;
    fn show_cursor(&mut self) -> io::Result<()>;
    fn leave_alternate_screen(&mut self) -> io::Result<()>;
}

struct CrosstermTerminal<W: Write> {
    output: W,
}

impl<W: Write> CrosstermTerminal<W> {
    fn new(output: W) -> Self {
        Self { output }
    }
}

impl<W: Write> TerminalCommands for CrosstermTerminal<W> {
    fn enter_alternate_screen(&mut self) -> io::Result<()> {
        execute!(self.output, EnterAlternateScreen)
    }

    fn hide_cursor(&mut self) -> io::Result<()> {
        execute!(self.output, cursor::Hide)
    }

    fn clear(&mut self) -> io::Result<()> {
        execute!(self.output, Clear(ClearType::All))
    }

    fn move_home(&mut self) -> io::Result<()> {
        execute!(self.output, cursor::MoveTo(0, 0))
    }

    fn write_text(&mut self, text: &str) -> io::Result<()> {
        self.output.write_all(text.as_bytes())?;
        self.output.flush()
    }

    fn show_cursor(&mut self) -> io::Result<()> {
        execute!(self.output, cursor::Show)
    }

    fn leave_alternate_screen(&mut self) -> io::Result<()> {
        execute!(self.output, LeaveAlternateScreen)
    }
}

struct TerminalSession<T: TerminalCommands> {
    terminal: T,
}

impl<T: TerminalCommands> TerminalSession<T> {
    fn enter(terminal: T) -> Result<Self, CliError> {
        let mut session = Self { terminal };
        session
            .terminal
            .enter_alternate_screen()
            .map_err(CliError::WriteHumanOutput)?;
        session
            .terminal
            .hide_cursor()
            .map_err(CliError::WriteHumanOutput)?;
        session
            .terminal
            .clear()
            .map_err(CliError::WriteHumanOutput)?;
        session
            .terminal
            .move_home()
            .map_err(CliError::WriteHumanOutput)?;
        Ok(session)
    }

    fn draw_snapshot(
        &mut self,
        snapshot: &Snapshot,
        transport: Option<Transport>,
    ) -> Result<(), CliError> {
        self.draw_text(&render_frame(snapshot, transport))
    }

    fn draw_text(&mut self, text: &str) -> Result<(), CliError> {
        self.terminal
            .move_home()
            .map_err(CliError::WriteHumanOutput)?;
        self.terminal.clear().map_err(CliError::WriteHumanOutput)?;
        self.terminal
            .write_text(text)
            .map_err(CliError::WriteHumanOutput)
    }

    fn finish_signal(self, signal: io::Result<()>) -> Result<(), CliError> {
        drop(self);
        signal.map_err(CliError::PrinterMonitorSignal)
    }
}

impl<T: TerminalCommands> Drop for TerminalSession<T> {
    fn drop(&mut self) {
        let _ = self.terminal.show_cursor();
        let _ = self.terminal.leave_alternate_screen();
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
        frame.push_str(&format!("Warning: {}\n", escape_terminal_controls(warning)));
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
        frame.push_str(&format!(
            "\n{} {} {} {} {}\n",
            format_cell("NAME", NAME_WIDTH),
            format_cell("STATUS", STATUS_WIDTH),
            format_cell("TRANSPORT", TRANSPORT_WIDTH),
            format_cell("CONNECTION", CONNECTION_WIDTH),
            format_cell("PROFILE", PROFILE_WIDTH),
        ));
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
        "{} {} {} {} {}\n",
        format_cell(&printer.name, NAME_WIDTH),
        format_cell(status, STATUS_WIDTH),
        format_cell(transport_label(printer.transport), TRANSPORT_WIDTH),
        format_cell(&connection_summary(&printer.connection), CONNECTION_WIDTH),
        format_cell(
            printer.profile.as_deref().unwrap_or("unassigned"),
            PROFILE_WIDTH
        ),
    )
}

fn format_cell(value: &str, width: usize) -> String {
    let sanitized = value.chars().map(replace_control).collect::<String>();
    let cells = display_width(&sanitized);
    if cells <= width {
        return format!("{sanitized}{:width$}", "", width = width - cells);
    }
    let marker = if width >= 3 { "..." } else { "" };
    let available = width - display_width(marker);
    let mut result = String::new();
    let mut used = 0;
    for character in sanitized.chars() {
        let character_width = UnicodeWidthChar::width(character).unwrap_or(0);
        if used + character_width > available {
            break;
        }
        result.push(character);
        used += character_width;
    }
    result.push_str(marker);
    format!(
        "{result}{:width$}",
        "",
        width = width - display_width(&result)
    )
}

fn replace_control(character: char) -> char {
    if character.is_control() {
        ' '
    } else {
        character
    }
}

fn escape_terminal_controls(value: &str) -> String {
    value
        .chars()
        .map(|character| match character {
            '\n' => "\\n".to_owned(),
            '\r' => "\\r".to_owned(),
            '\t' => "\\t".to_owned(),
            '\x1b' => "\\x1b".to_owned(),
            character if character.is_control() => format!("\\x{:02x}", character as u32),
            character => character.to_string(),
        })
        .collect()
}

fn display_width(value: &str) -> usize {
    value
        .chars()
        .map(|character| UnicodeWidthChar::width(character).unwrap_or(0))
        .sum()
}

fn transport_label(transport: Transport) -> &'static str {
    match transport {
        Transport::Usb => "usb",
        Transport::Network => "network",
    }
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
    use std::cell::RefCell;
    use std::io;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::rc::Rc;

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
        assert!(frame.contains("TRANSPORT"));
        assert!(frame.contains("PROFILE"));
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
    fn frame_escapes_warning_controls_without_creating_terminal_rows() {
        let frame = render_frame(
            &Snapshot {
                warning: Some("config\npath\r\ttab\x1b[2J\u{0007}".to_owned()),
                ..snapshot()
            },
            None,
        );

        assert!(frame.contains("Warning: config\\npath\\r\\ttab\\x1b[2J\\x07"));
        assert!(!frame.contains("Warning: config\npath"));
        assert!(!frame.contains('\x1b'));
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

    #[test]
    fn cells_use_display_width_and_replace_terminal_controls() {
        assert_eq!(format_cell("厨房\u{0301}\n", 8), "厨房́    ");
        assert_eq!(format_cell("abcdefghijk", 8), "abcde...");
    }

    #[test]
    fn frame_uses_the_approved_fixed_column_order() {
        let frame = render_frame(&snapshot(), None);
        let header = frame.lines().find(|line| line.starts_with("NAME")).unwrap();

        assert_eq!(
            header.split_whitespace().collect::<Vec<_>>(),
            ["NAME", "STATUS", "TRANSPORT", "CONNECTION", "PROFILE"]
        );
    }

    #[test]
    fn frame_uses_fixed_columns_for_long_names_and_profiles() {
        let frame = render_frame(
            &Snapshot {
                printers: vec![Printer {
                    name: "this-is-a-very-long-printer-name".to_owned(),
                    transport: Transport::Network,
                    availability: Availability::Unavailable,
                    profile: Some("this-is-a-very-long-profile".to_owned()),
                    connection: ConnectionFacts::Network(NetworkConnectionFacts {
                        host: "192.168.1.40".to_owned(),
                        port: 9100,
                    }),
                }],
                ..snapshot()
            },
            None,
        );

        assert!(frame.contains("NAME                 STATUS       TRANSPORT  CONNECTION"));
        let row = frame
            .lines()
            .find(|line| line.starts_with("this-is-a-very-lo..."))
            .expect("the long printer should have a row");
        assert_eq!(&row[..20], "this-is-a-very-lo...");
        assert_eq!(&row[21..33], "unavailable ");
        assert_eq!(&row[34..44], "network   ");
        assert!(row[45..75].starts_with("192.168.1.40:9100"));
        assert!(row[76..].starts_with("this-is-a-ver..."));
    }

    #[test]
    fn frame_lists_all_transport_and_profile_values() {
        let frame = render_frame(&snapshot(), None);

        assert!(frame.contains("kitchen              connected    network"));
        assert!(frame.contains("counter              connected    usb"));
    }

    #[test]
    fn terminal_session_emits_enter_redraw_and_normal_drop_commands() {
        let events = Rc::new(RefCell::new(Vec::new()));
        {
            let mut session = TerminalSession::enter(RecordingTerminal::new(events.clone(), false))
                .expect("the terminal session should enter");
            session
                .draw_text("snapshot")
                .expect("the frame should be drawn");
        }

        assert_eq!(
            *events.borrow(),
            [
                "enter", "hide", "clear", "home", "home", "clear", "write", "show", "leave"
            ]
        );
    }

    #[test]
    fn terminal_session_restores_after_a_write_error() {
        let events = Rc::new(RefCell::new(Vec::new()));
        {
            let mut session = TerminalSession::enter(RecordingTerminal::new(events.clone(), true))
                .expect("the terminal session should enter");
            assert!(session.draw_text("snapshot").is_err());
        }

        assert_eq!(
            *events.borrow(),
            [
                "enter", "hide", "clear", "home", "home", "clear", "write", "show", "leave"
            ]
        );
    }

    #[test]
    fn terminal_session_restores_when_the_monitor_exits_for_a_signal_error() {
        let events = Rc::new(RefCell::new(Vec::new()));

        let error = TerminalSession::enter(RecordingTerminal::new(events.clone(), false))
            .expect("the terminal session should enter")
            .finish_signal(Err(io::Error::other("injected signal failure")))
            .expect_err("a failed signal wait should be reported");

        assert!(matches!(error, CliError::PrinterMonitorSignal(_)));
        assert_eq!(
            *events.borrow(),
            ["enter", "hide", "clear", "home", "show", "leave"]
        );
    }

    #[test]
    fn terminal_session_drop_restores_the_terminal_while_unwinding() {
        let events = Rc::new(RefCell::new(Vec::new()));

        let result = catch_unwind(AssertUnwindSafe(|| {
            let _session = TerminalSession::enter(RecordingTerminal::new(events.clone(), false))
                .expect("the terminal session should enter");
            panic!("simulate an unexpected monitor failure");
        }));

        assert!(result.is_err());
        assert_eq!(
            *events.borrow(),
            ["enter", "hide", "clear", "home", "show", "leave"]
        );
    }

    #[test]
    fn crossterm_terminal_writes_the_exact_lifecycle_sequences() {
        let bytes = Rc::new(RefCell::new(Vec::new()));
        {
            let mut session =
                TerminalSession::enter(CrosstermTerminal::new(SharedBytes(bytes.clone())))
                    .expect("the terminal session should enter");
            session
                .draw_text("frame")
                .expect("the frame should be drawn");
        }

        assert_eq!(
            bytes.borrow().as_slice(),
            b"\x1b[?1049h\x1b[?25l\x1b[2J\x1b[1;1H\x1b[1;1H\x1b[2Jframe\x1b[?25h\x1b[?1049l"
        );
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

    struct RecordingTerminal {
        events: Rc<RefCell<Vec<&'static str>>>,
        fail_write: bool,
    }

    impl RecordingTerminal {
        fn new(events: Rc<RefCell<Vec<&'static str>>>, fail_write: bool) -> Self {
            Self { events, fail_write }
        }

        fn record(&self, event: &'static str) {
            self.events.borrow_mut().push(event);
        }
    }

    impl TerminalCommands for RecordingTerminal {
        fn enter_alternate_screen(&mut self) -> io::Result<()> {
            self.record("enter");
            Ok(())
        }

        fn hide_cursor(&mut self) -> io::Result<()> {
            self.record("hide");
            Ok(())
        }

        fn clear(&mut self) -> io::Result<()> {
            self.record("clear");
            Ok(())
        }

        fn move_home(&mut self) -> io::Result<()> {
            self.record("home");
            Ok(())
        }

        fn write_text(&mut self, _text: &str) -> io::Result<()> {
            self.record("write");
            if self.fail_write {
                return Err(io::Error::other("injected write failure"));
            }
            Ok(())
        }

        fn show_cursor(&mut self) -> io::Result<()> {
            self.record("show");
            Ok(())
        }

        fn leave_alternate_screen(&mut self) -> io::Result<()> {
            self.record("leave");
            Ok(())
        }
    }

    struct SharedBytes(Rc<RefCell<Vec<u8>>>);

    impl io::Write for SharedBytes {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.borrow_mut().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
}
