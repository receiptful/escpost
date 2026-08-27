//! Experimental command tracing model and crate-private collection seam.

use std::ops::Range;

use crate::RenderError;
use crate::state::Justification as StateJustification;
use crate::state::PrinterState;
use crate::surface::RenderSurface;
use crate::surface::tracing::TracingSurface;

/// Experimental semantic interpretation of a traced command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodedCommand {
    HorizontalTab,
    CarriageReturn,
    Initialize,
    SetRightSideCharacterSpacing(u8),
    SetAbsolutePrintPosition(u16),
    SelectPrintMode(u8),
    SelectBitImageMode {
        mode: u8,
        columns: u16,
    },
    SetUnderline(u8),
    SelectDefaultLineSpacing,
    SetLineSpacing(u8),
    SetHorizontalTabPositions(Vec<u8>),
    SetEmphasis(bool),
    PrintAndFeedPaper(u8),
    SelectCharacterFont(u8),
    SelectInternationalCharacterSet(u8),
    SetRelativePrintPosition(i16),
    PrintAndFeedLines(u8),
    GeneratePulse {
        connector: u8,
        on_time: u8,
        off_time: u8,
    },
    SelectCodeTable {
        table: u8,
        /// The encoding the active printer profile maps the table to.
        encoding: Option<String>,
    },
    SetJustification(Justification),
    SelectCharacterSize(u8),
    SetReversePrint(bool),
    SelectHriPosition(u8),
    SetLeftMargin(u16),
    SetMotionUnits {
        horizontal: u8,
        vertical: u8,
    },
    SetPrintAreaWidth(u16),
    SelectHriFont(u8),
    SetBarcodeHeight(u8),
    SetBarcodeWidth(u8),
    PrintBarcode {
        /// The barcode system, as the Function B number that names it.
        system: u8,
        data: Vec<u8>,
    },
    CutPaper {
        full: bool,
        /// The feed before the cut, for the cut commands that feed.
        feed: Option<u8>,
    },
    PrintBufferedGraphics,
    StoreRasterGraphics {
        extended_length: bool,
        width_dots: u16,
        height_dots: u16,
    },
    SelectQrModel(u8),
    SetQrModuleSize(u8),
    /// The error correction level, as 0 for L, 1 for M, 2 for Q and 3 for H.
    SelectQrErrorCorrection(u8),
    StoreQrData(usize),
    TextByte(u8),
    LineFeed,
    RasterImage {
        width_dots: u32,
        height_dots: u32,
        horizontal_scale: u8,
        vertical_scale: u8,
    },
    /// A raster image the printer drops, because the line already has data.
    SkippedRasterImage,
    QrCode(Vec<u8>),
    Unknown(CommandCode),
}

/// Experimental protocol identity for a parsed command without a typed model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandCode {
    Control(u8),
    Esc(u8),
    Gs(u8),
}

/// The character font a text byte prints with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextFont {
    A,
    B,
}

/// The state a printer holds at power-on, and after `ESC @`.
///
/// A printer profile decides these, thus they belong to the job rather than to
/// any moment in it. A reader tells a style that was set from one that was
/// never touched by comparing against them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyleDefaults {
    pub line_spacing_dots: u32,
    pub code_page: u8,
    pub international_character_set: u8,
}

/// The printer state that decides how a text byte reaches the paper.
///
/// It holds no position and no data, thus it changes only where a command
/// changes it, and a trace carries it only there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextStyle {
    pub font: TextFont,
    pub emphasized: bool,
    /// Dots of underline below a character, and 0 for none.
    pub underline_thickness: u8,
    pub width_magnification: u8,
    pub height_magnification: u8,
    /// True while the printer prints light characters on a dark cell.
    pub reversed: bool,
    pub justification: Justification,
    pub code_page: u8,
    /// The encoding the printer profile maps the code page to.
    pub encoding: Option<String>,
    pub international_character_set: u8,
    /// Dots the printer adds to the right of each character.
    pub right_side_character_spacing_dots: u32,
    /// Dots a line feed moves the paper, where no character stands taller.
    pub line_spacing_dots: u32,
}

/// Experimental justification value used by command traces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Justification {
    Left,
    Center,
    Right,
}

impl From<StateJustification> for Justification {
    fn from(value: StateJustification) -> Self {
        match value {
            StateJustification::Left => Self::Left,
            StateJustification::Center => Self::Center,
            StateJustification::Right => Self::Right,
        }
    }
}

/// Experimental typed state transition caused by a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateChange {
    Justification {
        before: Justification,
        after: Justification,
    },
}

/// Experimental logical printer position in printer-dot coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    pub x: u32,
    pub y: u32,
}

/// Experimental logical drawing bounds in printer-dot coordinates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaintRegion {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Experimental typed effect of a traced command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    StateChange(StateChange),
    Motion { before: Position, after: Position },
    Paint { bounds: PaintRegion },
}

/// Experimental lifecycle of logical paint produced by a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaintLifecycle {
    /// Paint remains in the printer's current line buffer.
    Buffered,
    /// Paint reached the rendered roll.
    Committed,
}

/// The largest number of command bytes one trace entry keeps.
///
/// A command list shows only the start of a command, thus a raster image does
/// not have to keep a copy of its payload.
pub const TRACED_COMMAND_BYTES: usize = 40;

/// Experimental trace entry for one safely decoded command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandTrace {
    pub byte_range: Range<usize>,
    /// The text style after this command, where the command changed it. The
    /// first command of a job carries it as the style the job starts with.
    pub style: Option<TextStyle>,
    /// The start of the command, at most [`TRACED_COMMAND_BYTES`] long.
    pub bytes: Vec<u8>,
    pub command: DecodedCommand,
    pub paint_lifecycle: Option<PaintLifecycle>,
    pub effects: Vec<Effect>,
}

/// Experimental commands associated with one conceptual output sheet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SheetTrace {
    pub commands: Vec<CommandTrace>,
}

/// Experimental ordered command trace grouped by conceptual output sheet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trace {
    pub sheets: Vec<SheetTrace>,
}

pub(crate) trait CommandSink {
    const ENABLED: bool;

    fn begin_command(&mut self, sheet_index: usize, offset: usize);
    fn describe_command(&mut self, command: DecodedCommand, effects: Vec<Effect>);
    fn finish_command(
        &mut self,
        bytes: &[u8],
        paint_lifecycle: Option<PaintLifecycle>,
        style: TextStyle,
    );
}

#[inline]
pub(crate) fn execute_line_feed<S: RenderSurface, C: CommandSink>(
    state: &mut PrinterState<S>,
    command_sink: &mut C,
) -> Result<(), RenderError> {
    if C::ENABLED {
        let before = state.trace_line_feed_start_position();
        state.line_feed()?;
        let after = state.trace_position();
        command_sink.describe_command(
            DecodedCommand::LineFeed,
            (before != after)
                .then_some(Effect::Motion {
                    before: position(before),
                    after: position(after),
                })
                .into_iter()
                .collect(),
        );
    } else {
        state.line_feed()?;
    }
    Ok(())
}

#[inline]
pub(crate) fn execute_text_byte<S: RenderSurface, C: CommandSink>(
    state: &mut PrinterState<S>,
    command_sink: &mut C,
    byte: u8,
    offset: usize,
) -> Result<(), RenderError> {
    state.print_byte(byte, offset)?;
    if C::ENABLED {
        command_sink.describe_command(DecodedCommand::TextByte(byte), vec![]);
    }
    Ok(())
}

/// Describes one command from its own bytes, for the print-job command list.
pub(crate) fn describe(bytes: &[u8]) -> DecodedCommand {
    let described = match bytes[0] {
        0x09 => Some(DecodedCommand::HorizontalTab),
        0x0a => Some(DecodedCommand::LineFeed),
        0x0d => Some(DecodedCommand::CarriageReturn),
        0x1b => describe_esc(bytes),
        0x1d => describe_gs(bytes),
        _ => None,
    };
    described.unwrap_or_else(|| DecodedCommand::Unknown(unknown_code(bytes)))
}

fn describe_esc(bytes: &[u8]) -> Option<DecodedCommand> {
    let parameter = |index: usize| bytes.get(index).copied();
    let word = |index: usize| {
        Some(u16::from_le_bytes([
            parameter(index)?,
            parameter(index + 1)?,
        ]))
    };
    Some(match parameter(1)? {
        0x40 => DecodedCommand::Initialize,
        0x20 => DecodedCommand::SetRightSideCharacterSpacing(parameter(2)?),
        0x24 => DecodedCommand::SetAbsolutePrintPosition(word(2)?),
        0x21 => DecodedCommand::SelectPrintMode(parameter(2)?),
        0x2a => DecodedCommand::SelectBitImageMode {
            mode: parameter(2)?,
            columns: word(3)?,
        },
        0x2d => DecodedCommand::SetUnderline(digit_or_number(parameter(2)?)?),
        0x32 => DecodedCommand::SelectDefaultLineSpacing,
        0x33 => DecodedCommand::SetLineSpacing(parameter(2)?),
        0x44 => DecodedCommand::SetHorizontalTabPositions(tab_positions(bytes)),
        0x45 => DecodedCommand::SetEmphasis(parameter(2)? & 0x01 != 0),
        0x4a => DecodedCommand::PrintAndFeedPaper(parameter(2)?),
        0x4d => DecodedCommand::SelectCharacterFont(digit_or_number(parameter(2)?)?),
        0x52 => DecodedCommand::SelectInternationalCharacterSet(parameter(2)?),
        0x5c => DecodedCommand::SetRelativePrintPosition(word(2)? as i16),
        0x61 => DecodedCommand::SetJustification(match digit_or_number(parameter(2)?)? {
            0 => Justification::Left,
            1 => Justification::Center,
            2 => Justification::Right,
            _ => return None,
        }),
        0x64 => DecodedCommand::PrintAndFeedLines(parameter(2)?),
        0x70 => DecodedCommand::GeneratePulse {
            connector: digit_or_number(parameter(2)?)?,
            on_time: parameter(3)?,
            off_time: parameter(4)?,
        },
        0x74 => DecodedCommand::SelectCodeTable {
            table: parameter(2)?,
            encoding: None,
        },
        _ => return None,
    })
}

fn describe_gs(bytes: &[u8]) -> Option<DecodedCommand> {
    let parameter = |index: usize| bytes.get(index).copied();
    let word = |index: usize| {
        Some(u16::from_le_bytes([
            parameter(index)?,
            parameter(index + 1)?,
        ]))
    };
    Some(match parameter(1)? {
        0x21 => DecodedCommand::SelectCharacterSize(parameter(2)?),
        0x28 if parameter(2)? == b'L' => describe_graphics(bytes.get(5..)?, false)?,
        0x28 if parameter(2)? == b'k' => describe_qr(bytes.get(5..)?)?,
        0x38 if parameter(2)? == b'L' => describe_graphics(bytes.get(7..)?, true)?,
        0x42 => DecodedCommand::SetReversePrint(parameter(2)? & 0x01 != 0),
        0x48 => DecodedCommand::SelectHriPosition(digit_or_number(parameter(2)?)?),
        0x4c => DecodedCommand::SetLeftMargin(word(2)?),
        0x50 => DecodedCommand::SetMotionUnits {
            horizontal: parameter(2)?,
            vertical: parameter(3)?,
        },
        0x57 => DecodedCommand::SetPrintAreaWidth(word(2)?),
        0x66 => DecodedCommand::SelectHriFont(digit_or_number(parameter(2)?)?),
        0x68 => DecodedCommand::SetBarcodeHeight(parameter(2)?),
        0x77 => DecodedCommand::SetBarcodeWidth(parameter(2)?),
        0x6b => describe_barcode(bytes)?,
        0x56 => describe_cut(bytes)?,
        0x76 if parameter(2)? == b'0' => describe_raster_image(bytes)?,
        _ => return None,
    })
}

/// Describes `GS k`, whose data ends either with a byte count or with a NUL.
fn describe_barcode(bytes: &[u8]) -> Option<DecodedCommand> {
    let system = bytes.get(2).copied()?;
    let (system, data) = match system {
        function_a @ 0..=6 => {
            let data = bytes.get(3..)?;
            let end = data
                .iter()
                .position(|byte| *byte == 0)
                .unwrap_or(data.len());
            (function_a + 65, &data[..end])
        }
        function_b @ 65..=79 => (function_b, bytes.get(4..)?),
        _ => return None,
    };
    Some(DecodedCommand::PrintBarcode {
        system,
        data: data.to_vec(),
    })
}

/// Describes `GS V`, whose Function B and D forms feed before they cut.
fn describe_cut(bytes: &[u8]) -> Option<DecodedCommand> {
    let mode = bytes.get(2).copied()?;
    let (full, feeds) = match mode {
        0 | 48 => (true, false),
        1 | 49 => (false, false),
        65 | 103 => (true, true),
        66 | 104 => (false, true),
        _ => return None,
    };
    Some(DecodedCommand::CutPaper {
        full,
        feed: feeds.then(|| bytes.get(3).copied()).flatten(),
    })
}

/// Describes `GS v 0`, which the printer drops when the line already has data.
fn describe_raster_image(bytes: &[u8]) -> Option<DecodedCommand> {
    if bytes.len() == 3 {
        return Some(DecodedCommand::SkippedRasterImage);
    }
    let (horizontal_scale, vertical_scale) = match bytes.get(3).copied()? {
        0 | 48 => (1, 1),
        1 | 49 => (2, 1),
        2 | 50 => (1, 2),
        3 | 51 => (2, 2),
        _ => return None,
    };
    let width_bytes = u16::from_le_bytes([bytes.get(4).copied()?, bytes.get(5).copied()?]);
    let height_dots = u16::from_le_bytes([bytes.get(6).copied()?, bytes.get(7).copied()?]);
    Some(DecodedCommand::RasterImage {
        width_dots: u32::from(width_bytes).saturating_mul(8),
        height_dots: u32::from(height_dots),
        horizontal_scale,
        vertical_scale,
    })
}

/// Describes the `GS ( L` and `GS 8 L` functions the renderer draws.
fn describe_graphics(parameters: &[u8], extended_length: bool) -> Option<DecodedCommand> {
    Some(match parameters.get(1).copied()? {
        2 | 50 if !extended_length => DecodedCommand::PrintBufferedGraphics,
        112 => DecodedCommand::StoreRasterGraphics {
            extended_length,
            width_dots: u16::from_le_bytes([
                parameters.get(6).copied()?,
                parameters.get(7).copied()?,
            ]),
            height_dots: u16::from_le_bytes([
                parameters.get(8).copied()?,
                parameters.get(9).copied()?,
            ]),
        },
        _ => return None,
    })
}

/// Describes the `GS ( k` QR Code functions.
fn describe_qr(parameters: &[u8]) -> Option<DecodedCommand> {
    if parameters.first().copied()? != 49 {
        return None;
    }
    Some(match parameters.get(1).copied()? {
        65 => DecodedCommand::SelectQrModel(match parameters.get(2).copied()? {
            49 => 1,
            50 => 2,
            51 => 3,
            _ => return None,
        }),
        67 => DecodedCommand::SetQrModuleSize(parameters.get(2).copied()?),
        69 => {
            DecodedCommand::SelectQrErrorCorrection(digit_or_number(parameters.get(2).copied()?)?)
        }
        80 => DecodedCommand::StoreQrData(parameters.get(3..)?.len()),
        81 => DecodedCommand::QrCode(Vec::new()),
        _ => return None,
    })
}

/// Reads a parameter that Epson accepts both as a number and as its ASCII digit.
fn digit_or_number(value: u8) -> Option<u8> {
    match value {
        0..=9 => Some(value),
        b'0'..=b'9' => Some(value - b'0'),
        _ => None,
    }
}

/// Collects the tab columns of `ESC D`, without the terminator that ends them.
fn tab_positions(bytes: &[u8]) -> Vec<u8> {
    let mut columns = bytes.get(2..).unwrap_or_default().to_vec();
    if columns.last() == Some(&0) {
        columns.pop();
    }
    columns
}

fn unknown_code(bytes: &[u8]) -> CommandCode {
    match bytes[0] {
        0x1b => CommandCode::Esc(bytes.get(1).copied().unwrap_or_default()),
        0x1d => CommandCode::Gs(bytes.get(1).copied().unwrap_or_default()),
        byte => CommandCode::Control(byte),
    }
}

fn position((x, y): (u32, u32)) -> Position {
    Position { x, y }
}

pub(crate) struct NoTrace;

impl CommandSink for NoTrace {
    const ENABLED: bool = false;

    #[inline]
    fn begin_command(&mut self, _sheet_index: usize, _offset: usize) {
        unreachable!("NoTrace commands are guarded by CommandSink::ENABLED")
    }

    #[inline]
    fn describe_command(&mut self, _command: DecodedCommand, _effects: Vec<Effect>) {
        unreachable!("NoTrace commands are guarded by CommandSink::ENABLED")
    }

    #[inline]
    fn finish_command(
        &mut self,
        _bytes: &[u8],
        _paint_lifecycle: Option<PaintLifecycle>,
        _style: TextStyle,
    ) {
        unreachable!("NoTrace commands are guarded by CommandSink::ENABLED")
    }
}

#[derive(Debug)]
struct PendingCommand {
    sheet_index: usize,
    start_offset: usize,
    description: Option<(DecodedCommand, Vec<Effect>)>,
}

#[derive(Debug, Default)]
pub(crate) struct TraceCollector {
    commands: Vec<(usize, CommandTrace)>,
    pending: Option<PendingCommand>,
    /// The style the last command left behind, to tell a change from a repeat.
    style: Option<TextStyle>,
}

impl TraceCollector {
    pub(crate) fn finish(self, surfaces: &[TracingSurface]) -> Trace {
        let command_sheet_count = self
            .commands
            .iter()
            .map(|(sheet_index, _)| sheet_index.saturating_add(1))
            .max()
            .unwrap_or_default();
        let sheet_count = surfaces.len().max(command_sheet_count);
        let mut sheets = (0..sheet_count)
            .map(|_| SheetTrace {
                commands: Vec::new(),
            })
            .collect::<Vec<_>>();

        for (sheet_index, mut command) in self.commands {
            if let Some(surface) = surfaces.get(sheet_index)
                && let Some(bounds) = command_bounds(surface, command.byte_range.start)
            {
                command.paint_lifecycle = Some(PaintLifecycle::Committed);
                command.effects.push(Effect::Paint { bounds });
            }
            if let Some(sheet) = sheets.get_mut(sheet_index) {
                sheet.commands.push(command);
            }
        }

        Trace { sheets }
    }
}

fn command_bounds(surface: &TracingSurface, command_offset: usize) -> Option<PaintRegion> {
    let mut regions = surface
        .logical_regions
        .iter()
        .filter(|region| region.command_offset == command_offset);
    let first = regions.next()?;
    let mut left = first.x;
    let mut top = first.y;
    let mut right = first.x.saturating_add(first.width);
    let mut bottom = first.y.saturating_add(first.height);
    for region in regions {
        left = left.min(region.x);
        top = top.min(region.y);
        right = right.max(region.x.saturating_add(region.width));
        bottom = bottom.max(region.y.saturating_add(region.height));
    }
    Some(PaintRegion {
        x: left,
        y: top,
        width: right.saturating_sub(left),
        height: bottom.saturating_sub(top),
    })
}

impl CommandSink for TraceCollector {
    const ENABLED: bool = true;

    fn begin_command(&mut self, sheet_index: usize, offset: usize) {
        debug_assert!(self.pending.is_none());
        self.pending = Some(PendingCommand {
            sheet_index,
            start_offset: offset,
            description: None,
        });
    }

    fn describe_command(&mut self, command: DecodedCommand, effects: Vec<Effect>) {
        let pending = self
            .pending
            .as_mut()
            .expect("traced command descriptions require an active command");
        debug_assert!(pending.description.is_none());
        pending.description = Some((command, effects));
    }

    fn finish_command(
        &mut self,
        bytes: &[u8],
        paint_lifecycle: Option<PaintLifecycle>,
        style: TextStyle,
    ) {
        let pending = self
            .pending
            .take()
            .expect("traced command finalization requires an active command");
        let (command, effects) = pending
            .description
            .unwrap_or_else(|| (describe(bytes), vec![]));
        // The first command carries the style the job starts with. Every later
        // command carries it only where it changed, thus a reader of the trace
        // holds the last style it saw.
        let changed = self.style.as_ref() != Some(&style);
        if changed {
            self.style = Some(style.clone());
        }
        self.commands.push((
            pending.sheet_index,
            CommandTrace {
                byte_range: pending.start_offset..pending.start_offset + bytes.len(),
                bytes: bytes[..bytes.len().min(TRACED_COMMAND_BYTES)].to_vec(),
                style: changed.then_some(style),
                command,
                paint_lifecycle,
                effects,
            },
        ));
    }
}

#[cfg(test)]
mod describe_tests {
    use super::{CommandCode, DecodedCommand, Justification, describe};

    #[test]
    fn control_bytes_describe_their_own_function() {
        assert_eq!(describe(&[0x09]), DecodedCommand::HorizontalTab);
        assert_eq!(describe(&[0x0a]), DecodedCommand::LineFeed);
        assert_eq!(describe(&[0x0d]), DecodedCommand::CarriageReturn);
    }

    #[test]
    fn esc_commands_describe_their_parameters() {
        assert_eq!(describe(&[0x1b, b'@']), DecodedCommand::Initialize);
        assert_eq!(
            describe(&[0x1b, b' ', 3]),
            DecodedCommand::SetRightSideCharacterSpacing(3)
        );
        assert_eq!(
            describe(&[0x1b, b'$', 0x30, 0x01]),
            DecodedCommand::SetAbsolutePrintPosition(304)
        );
        assert_eq!(
            describe(&[0x1b, b'!', 0x38]),
            DecodedCommand::SelectPrintMode(0x38)
        );
        assert_eq!(
            describe(&[0x1b, b'*', 1, 2, 0, 0x0f, 0xf0]),
            DecodedCommand::SelectBitImageMode {
                mode: 1,
                columns: 2
            }
        );
        assert_eq!(describe(&[0x1b, b'-', 50]), DecodedCommand::SetUnderline(2));
        assert_eq!(
            describe(&[0x1b, b'2']),
            DecodedCommand::SelectDefaultLineSpacing
        );
        assert_eq!(
            describe(&[0x1b, b'3', 24]),
            DecodedCommand::SetLineSpacing(24)
        );
        assert_eq!(
            describe(&[0x1b, b'D', 8, 16, 24, 0]),
            DecodedCommand::SetHorizontalTabPositions(vec![8, 16, 24])
        );
        assert_eq!(
            describe(&[0x1b, b'E', 1]),
            DecodedCommand::SetEmphasis(true)
        );
        assert_eq!(
            describe(&[0x1b, b'E', 0]),
            DecodedCommand::SetEmphasis(false)
        );
        assert_eq!(
            describe(&[0x1b, b'J', 30]),
            DecodedCommand::PrintAndFeedPaper(30)
        );
        assert_eq!(
            describe(&[0x1b, b'M', 49]),
            DecodedCommand::SelectCharacterFont(1)
        );
        assert_eq!(
            describe(&[0x1b, b'R', 2]),
            DecodedCommand::SelectInternationalCharacterSet(2)
        );
        assert_eq!(
            describe(&[0x1b, b'\\', 0xec, 0xff]),
            DecodedCommand::SetRelativePrintPosition(-20)
        );
        assert_eq!(
            describe(&[0x1b, b'a', 49]),
            DecodedCommand::SetJustification(Justification::Center)
        );
        assert_eq!(
            describe(&[0x1b, b'd', 3]),
            DecodedCommand::PrintAndFeedLines(3)
        );
        assert_eq!(
            describe(&[0x1b, b'p', 48, 25, 50]),
            DecodedCommand::GeneratePulse {
                connector: 0,
                on_time: 25,
                off_time: 50
            }
        );
        assert_eq!(
            describe(&[0x1b, b't', 16]),
            DecodedCommand::SelectCodeTable {
                table: 16,
                encoding: None
            }
        );
    }

    #[test]
    fn gs_commands_describe_their_parameters() {
        assert_eq!(
            describe(&[0x1d, b'!', 0x11]),
            DecodedCommand::SelectCharacterSize(0x11)
        );
        assert_eq!(
            describe(&[0x1d, b'B', 1]),
            DecodedCommand::SetReversePrint(true)
        );
        assert_eq!(
            describe(&[0x1d, b'H', 50]),
            DecodedCommand::SelectHriPosition(2)
        );
        assert_eq!(
            describe(&[0x1d, b'L', 0x18, 0x00]),
            DecodedCommand::SetLeftMargin(24)
        );
        assert_eq!(
            describe(&[0x1d, b'P', 180, 360u16 as u8]),
            DecodedCommand::SetMotionUnits {
                horizontal: 180,
                vertical: 104
            }
        );
        assert_eq!(
            describe(&[0x1d, b'W', 0x00, 0x02]),
            DecodedCommand::SetPrintAreaWidth(512)
        );
        assert_eq!(
            describe(&[0x1d, b'f', 49]),
            DecodedCommand::SelectHriFont(1)
        );
        assert_eq!(
            describe(&[0x1d, b'h', 80]),
            DecodedCommand::SetBarcodeHeight(80)
        );
        assert_eq!(
            describe(&[0x1d, b'w', 3]),
            DecodedCommand::SetBarcodeWidth(3)
        );
    }

    #[test]
    fn cut_commands_describe_their_shape_and_feed() {
        assert_eq!(
            describe(&[0x1d, b'V', 48]),
            DecodedCommand::CutPaper {
                full: true,
                feed: None
            }
        );
        assert_eq!(
            describe(&[0x1d, b'V', 1]),
            DecodedCommand::CutPaper {
                full: false,
                feed: None
            }
        );
        assert_eq!(
            describe(&[0x1d, b'V', 66, 24]),
            DecodedCommand::CutPaper {
                full: false,
                feed: Some(24)
            }
        );
    }

    #[test]
    fn barcode_commands_carry_their_system_and_data() {
        assert_eq!(
            describe(&[0x1d, b'k', 73, 6, b'{', b'B', b'1', b'2', b'3', b'4']),
            DecodedCommand::PrintBarcode {
                system: 73,
                data: b"{B1234".to_vec()
            }
        );
        assert_eq!(
            describe(&[0x1d, b'k', 4, b'*', b'1', b'2', b'*', 0]),
            DecodedCommand::PrintBarcode {
                system: 69,
                data: b"*12*".to_vec()
            }
        );
    }

    #[test]
    fn graphics_commands_describe_their_function() {
        assert_eq!(
            describe(&[0x1d, b'(', b'L', 2, 0, 48, 50]),
            DecodedCommand::PrintBufferedGraphics
        );
        assert_eq!(
            describe(&[
                0x1d, b'(', b'L', 14, 0, 48, 112, 48, 1, 1, 49, 16, 0, 2, 0, 0, 0, 0, 0
            ]),
            DecodedCommand::StoreRasterGraphics {
                extended_length: false,
                width_dots: 16,
                height_dots: 2
            }
        );
        assert_eq!(
            describe(&[
                0x1d, b'8', b'L', 14, 0, 0, 0, 48, 112, 48, 1, 1, 49, 16, 0, 2, 0, 0, 0, 0, 0
            ]),
            DecodedCommand::StoreRasterGraphics {
                extended_length: true,
                width_dots: 16,
                height_dots: 2
            }
        );
    }

    #[test]
    fn raster_images_describe_their_dot_size() {
        assert_eq!(
            describe(&[0x1d, b'v', b'0', 0, 2, 0, 3, 0, 0, 0, 0, 0, 0, 0]),
            DecodedCommand::RasterImage {
                width_dots: 16,
                height_dots: 3,
                horizontal_scale: 1,
                vertical_scale: 1
            }
        );
        assert_eq!(
            describe(&[0x1d, b'v', b'0', 3, 1, 0, 1, 0, 0]),
            DecodedCommand::RasterImage {
                width_dots: 8,
                height_dots: 1,
                horizontal_scale: 2,
                vertical_scale: 2
            }
        );
    }

    #[test]
    fn a_raster_image_the_printer_skips_is_named_as_skipped() {
        assert_eq!(
            describe(&[0x1d, b'v', b'0']),
            DecodedCommand::SkippedRasterImage
        );
    }

    #[test]
    fn qr_functions_describe_themselves_separately() {
        assert_eq!(
            describe(&[0x1d, b'(', b'k', 4, 0, 49, 65, 50, 0]),
            DecodedCommand::SelectQrModel(2)
        );
        assert_eq!(
            describe(&[0x1d, b'(', b'k', 3, 0, 49, 67, 4]),
            DecodedCommand::SetQrModuleSize(4)
        );
        assert_eq!(
            describe(&[0x1d, b'(', b'k', 3, 0, 49, 69, 49]),
            DecodedCommand::SelectQrErrorCorrection(1)
        );
        assert_eq!(
            describe(&[0x1d, b'(', b'k', 5, 0, 49, 80, 48, b'h', b'i']),
            DecodedCommand::StoreQrData(2)
        );
        assert_eq!(
            describe(&[0x1d, b'(', b'k', 3, 0, 49, 81, 48]),
            DecodedCommand::QrCode(Vec::new())
        );
    }

    #[test]
    fn a_truncated_command_stays_unknown_instead_of_guessing() {
        assert_eq!(
            describe(&[0x1b, b'3']),
            DecodedCommand::Unknown(CommandCode::Esc(b'3'))
        );
    }

    #[test]
    fn an_unlisted_command_keeps_only_its_protocol_identity() {
        assert_eq!(
            describe(&[0x1b, b'5']),
            DecodedCommand::Unknown(CommandCode::Esc(b'5'))
        );
    }
}

#[cfg(test)]
mod tests {
    use escpost_profiles::compile_profile;

    use super::{
        CommandTrace, DecodedCommand, Effect, Justification, PaintLifecycle, Position, StateChange,
        TraceCollector,
    };
    use crate::surface::tracing::TracingSurface;
    use crate::{RenderOptions, render, render_surfaces_with_sink};

    const CAPABILITIES_JSON: &[u8] = include_bytes!("../tests/fixtures/capabilities.json");
    const PROFILE_TOML: &str = include_str!("../tests/fixtures/profile.toml");

    #[test]
    fn traced_render_attributes_centered_text_to_its_input_byte() {
        let profile = compile_profile(CAPABILITIES_JSON, PROFILE_TOML)
            .expect("the fictional renderer test profile should compile");
        let input = [0x1b, b'a', 1, b'A', 0x0a];

        let ordinary = render(&input, &profile).expect("ordinary rendering should succeed");
        let mut commands = TraceCollector::default();
        let traced = render_surfaces_with_sink::<TracingSurface, _>(
            &input,
            &profile,
            &RenderOptions::default(),
            &mut commands,
        )
        .expect("traced rendering should succeed");
        let traced_sheet = &traced.surfaces[0];
        let trace = commands.finish(&traced.surfaces);
        let commands = &trace.sheets[0].commands;

        assert_eq!(
            commands[0],
            CommandTrace {
                byte_range: 0..3,
                bytes: vec![0x1b, b'a', 1],
                style: commands[0].style.clone(),
                command: DecodedCommand::SetJustification(Justification::Center),
                paint_lifecycle: None,
                effects: vec![Effect::StateChange(StateChange::Justification {
                    before: Justification::Left,
                    after: Justification::Center,
                })],
            }
        );
        assert_eq!(commands[1].byte_range, 3..4);
        assert_eq!(commands[1].command, DecodedCommand::TextByte(b'A'));
        assert_eq!(commands[1].paint_lifecycle, Some(PaintLifecycle::Committed));
        let [Effect::Paint { bounds }] = commands[1].effects.as_slice() else {
            panic!("the printable byte should have exactly one paint effect");
        };
        assert_eq!(
            (bounds.x, bounds.y, bounds.width, bounds.height),
            (186, 0, 12, 24)
        );
        assert_eq!(
            commands[2],
            CommandTrace {
                byte_range: 4..5,
                bytes: vec![0x0a],
                style: None,
                command: DecodedCommand::LineFeed,
                paint_lifecycle: None,
                effects: vec![Effect::Motion {
                    before: Position { x: 198, y: 0 },
                    after: Position { x: 0, y: 30 },
                }],
            }
        );

        assert_eq!(traced_sheet.inner, ordinary.sheets[0].surface);
        let text_bounds = traced_sheet
            .logical_regions
            .iter()
            .filter(|region| region.command_offset == 3)
            .collect::<Vec<_>>();
        assert!(!text_bounds.is_empty());
        assert!(
            text_bounds
                .iter()
                .all(|region| region.x >= 186 && region.x < 198)
        );
        assert!(
            traced_sheet
                .logical_regions
                .iter()
                .all(|region| region.command_offset != 4),
            "LF must move the text without taking ownership of its pixels"
        );
    }

    #[test]
    fn undrawn_paint_commands_have_no_fabricated_bounds() {
        let profile = compile_profile(CAPABILITIES_JSON, PROFILE_TOML)
            .expect("the fictional renderer test profile should compile");
        let input = [0x1b, b'*', 1, 1, 0, 0xff, 0x0a];
        let mut commands = TraceCollector::default();

        let traced = render_surfaces_with_sink::<TracingSurface, _>(
            &input,
            &profile,
            &RenderOptions::default(),
            &mut commands,
        )
        .expect("traced rendering should succeed");

        let trace = commands.finish(&traced.surfaces);
        let command = &trace.sheets[0].commands[0];
        assert_eq!(
            command.command,
            DecodedCommand::SelectBitImageMode {
                mode: 1,
                columns: 1
            }
        );
        assert!(command.effects.is_empty());
        assert!(
            traced
                .surfaces
                .iter()
                .all(|surface| surface.logical_regions.is_empty()),
            "unsupported paint commands must not retain logical regions"
        );
    }
}
