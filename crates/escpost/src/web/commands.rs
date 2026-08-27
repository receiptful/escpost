//! Names and describes each command of a print job for the command list.

use escpost_render::{
    CommandCode, CommandTrace, DecodedCommand, Effect, Justification, PaintLifecycle, StateChange,
    TextFont, TextStyle,
};
use serde::Serialize;

/// How many parameter bytes one command shows before the count stands for the
/// rest. A command that carries data, such as a raster image, would otherwise
/// fill the command list with its payload.
const PARAMETER_BYTES_SHOWN: usize = 8;

#[derive(Clone, Serialize)]
pub(crate) struct CommandResponse {
    byte_start: usize,
    byte_end: usize,
    name: String,
    detail: String,
    /// The bytes that name the command, as uppercase hexadecimal.
    code_bytes: String,
    /// The first parameter bytes, as uppercase hexadecimal.
    capped_parameter_bytes: String,
    /// How many parameter bytes the command has in total.
    total_parameter_bytes: usize,
    /// True when the command itself fixes how many parameter bytes follow,
    /// thus the command list can show them beside the command name.
    fixed_parameters: bool,
    /// The text style after the command, where the command changed it. Every
    /// later command prints with the last style a command carried.
    #[serde(skip_serializing_if = "Option::is_none")]
    text_style: Option<TextStyleResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    paint_lifecycle: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    annotation: Option<AnnotationResponse>,
    effects: Vec<EffectResponse>,
}

/// The name, description and command-byte count of one decoded command.
struct CommandDisplay {
    name: String,
    detail: String,
    /// How many leading bytes name the command instead of carrying data.
    code_length: usize,
}

pub(crate) fn command_responses(commands: Vec<CommandTrace>) -> Vec<CommandResponse> {
    commands
        .into_iter()
        .map(|command| {
            let shown = display(&command.command);
            let annotation = match &command.command {
                DecodedCommand::QrCode(data) => Some(qr_annotation(data)),
                _ => None,
            };
            let code_length = shown.code_length.min(command.bytes.len());
            let parameters = &command.bytes[code_length..];
            CommandResponse {
                byte_start: command.byte_range.start,
                byte_end: command.byte_range.end,
                name: shown.name,
                detail: shown.detail,
                code_bytes: hexadecimal(&command.bytes[..code_length]),
                capped_parameter_bytes: hexadecimal(
                    &parameters[..parameters.len().min(PARAMETER_BYTES_SHOWN)],
                ),
                total_parameter_bytes: command.byte_range.len().saturating_sub(code_length),
                fixed_parameters: fixed_parameters(&command.command),
                text_style: command.style.map(text_style_response),
                paint_lifecycle: command.paint_lifecycle.map(|lifecycle| match lifecycle {
                    PaintLifecycle::Buffered => "buffered",
                    PaintLifecycle::Committed => "committed",
                }),
                annotation,
                effects: command.effects.into_iter().map(effect_response).collect(),
            }
        })
        .collect()
}

/// Tells whether the command, and not its data, fixes its parameter count.
fn fixed_parameters(command: &DecodedCommand) -> bool {
    !matches!(
        command,
        DecodedCommand::SelectBitImageMode { .. }
            | DecodedCommand::SetHorizontalTabPositions(_)
            | DecodedCommand::PrintBarcode { .. }
            | DecodedCommand::StoreRasterGraphics { .. }
            | DecodedCommand::StoreQrData(_)
            | DecodedCommand::RasterImage { .. }
            | DecodedCommand::TextByte(_)
            | DecodedCommand::Unknown(_)
    )
}

fn hexadecimal(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Names and describes one command, as the Epson command manual does.
fn display(command: &DecodedCommand) -> CommandDisplay {
    let (name, detail, code_length) = match command {
        DecodedCommand::HorizontalTab => ("HT", "Horizontal tab".to_owned(), 1),
        DecodedCommand::LineFeed => ("LF", "Print and line feed".to_owned(), 1),
        DecodedCommand::CarriageReturn => ("CR", "Print and carriage return".to_owned(), 1),
        DecodedCommand::Initialize => ("ESC @", "Initialize printer".to_owned(), 2),
        DecodedCommand::SetRightSideCharacterSpacing(spacing) => (
            "ESC SP",
            format!("Set right-side character spacing: {spacing} × horizontal motion unit"),
            2,
        ),
        DecodedCommand::SetAbsolutePrintPosition(position) => (
            "ESC $",
            format!("Set absolute print position: {position} × horizontal motion unit"),
            2,
        ),
        DecodedCommand::SelectPrintMode(mode) => (
            "ESC !",
            format!("Set print mode(s): {}", print_mode(*mode)),
            2,
        ),
        DecodedCommand::SelectBitImageMode { mode, columns } => (
            "ESC *",
            format!(
                "Set bit-image mode: {} · {columns} dots wide",
                bit_image_mode(*mode)
            ),
            2,
        ),
        DecodedCommand::SetUnderline(thickness) => (
            "ESC -",
            format!(
                "Turn underline mode: {}",
                match thickness {
                    0 => "off".to_owned(),
                    1 => "on, 1 dot thick".to_owned(),
                    thickness => format!("on, {thickness} dots thick"),
                }
            ),
            2,
        ),
        DecodedCommand::SelectDefaultLineSpacing => {
            ("ESC 2", "Set default line spacing".to_owned(), 2)
        }
        DecodedCommand::SetLineSpacing(spacing) => (
            "ESC 3",
            format!("Set line spacing: {spacing} × vertical motion unit"),
            2,
        ),
        DecodedCommand::SetHorizontalTabPositions(columns) => (
            "ESC D",
            format!(
                "Set horizontal tab positions: {}",
                if columns.is_empty() {
                    "cleared".to_owned()
                } else {
                    format!(
                        "{} {}",
                        if columns.len() == 1 {
                            "column"
                        } else {
                            "columns"
                        },
                        columns
                            .iter()
                            .map(u8::to_string)
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                }
            ),
            2,
        ),
        DecodedCommand::SetEmphasis(on) => {
            ("ESC E", format!("Turn emphasized mode: {}", switch(*on)), 2)
        }
        DecodedCommand::PrintAndFeedPaper(distance) => (
            "ESC J",
            format!("Print and feed paper: {distance} × vertical motion unit"),
            2,
        ),
        DecodedCommand::SelectCharacterFont(font) => (
            "ESC M",
            format!("Set character font: {}", font_name(*font)),
            2,
        ),
        DecodedCommand::SelectInternationalCharacterSet(set) => (
            "ESC R",
            format!(
                "Set international character set: {}",
                international_character_set(*set)
            ),
            2,
        ),
        DecodedCommand::SetRelativePrintPosition(position) => (
            "ESC \\",
            format!("Set relative print position: {position} × horizontal motion unit"),
            2,
        ),
        DecodedCommand::SetJustification(justification) => (
            "ESC a",
            format!(
                "Set justification: {}",
                match justification {
                    Justification::Left => "left",
                    Justification::Center => "centered",
                    Justification::Right => "right",
                }
            ),
            2,
        ),
        DecodedCommand::PrintAndFeedLines(lines) => (
            "ESC d",
            format!(
                "Print and feed n lines: {lines} {}",
                if *lines == 1 { "line" } else { "lines" }
            ),
            2,
        ),
        DecodedCommand::GeneratePulse {
            connector,
            on_time,
            off_time,
        } => (
            "ESC p",
            format!(
                "Generate pulse: drawer kick-out connector pin {} · {} ms on · {} ms off",
                if *connector == 0 { 2 } else { 5 },
                u16::from(*on_time) * 2,
                u16::from(*off_time) * 2
            ),
            2,
        ),
        DecodedCommand::SelectCodeTable { table, encoding } => (
            "ESC t",
            format!(
                "Set character code table: {}",
                match encoding {
                    Some(encoding) => encoding.clone(),
                    None => format!("page {table}"),
                }
            ),
            2,
        ),
        DecodedCommand::SelectCharacterSize(size) => (
            "GS !",
            format!(
                "Set character size: {} × width · {} × height",
                ((size >> 4) & 0x07) + 1,
                (size & 0x07) + 1
            ),
            2,
        ),
        DecodedCommand::SetReversePrint(on) => (
            "GS B",
            format!("Turn white/black reverse print mode: {}", switch(*on)),
            2,
        ),
        DecodedCommand::SelectHriPosition(position) => (
            "GS H",
            format!(
                "Set print position of HRI characters: {}",
                match position {
                    0 => "not printed",
                    1 => "above the barcode",
                    2 => "below the barcode",
                    _ => "above and below the barcode",
                }
            ),
            2,
        ),
        DecodedCommand::SetLeftMargin(margin) => (
            "GS L",
            format!("Set left margin: {margin} × horizontal motion unit"),
            2,
        ),
        DecodedCommand::SetMotionUnits {
            horizontal,
            vertical,
        } => (
            "GS P",
            format!(
                "Set horizontal and vertical motion units: {} × {}",
                motion_unit(*horizontal),
                motion_unit(*vertical)
            ),
            2,
        ),
        DecodedCommand::SetPrintAreaWidth(width) => (
            "GS W",
            format!("Set print area width: {width} × horizontal motion unit"),
            2,
        ),
        DecodedCommand::SelectHriFont(font) => (
            "GS f",
            format!("Set font for HRI characters: {}", font_name(*font)),
            2,
        ),
        DecodedCommand::SetBarcodeHeight(height) => {
            ("GS h", format!("Set barcode height: {height} dots"), 2)
        }
        DecodedCommand::SetBarcodeWidth(width) => {
            ("GS w", format!("Set barcode width: {width} dots"), 2)
        }
        DecodedCommand::PrintBarcode { system, data } => (
            "GS k",
            format!(
                "Print barcode: {} · {}",
                barcode_system(*system),
                readable(data)
            ),
            2,
        ),
        DecodedCommand::CutPaper { full, feed } => (
            "GS V",
            format!(
                "Set cut mode and cut paper: {} cut{}",
                if *full { "full" } else { "partial" },
                match feed {
                    Some(feed) => format!(" · feeds {feed} × vertical motion unit first"),
                    None => String::new(),
                }
            ),
            2,
        ),
        DecodedCommand::PrintBufferedGraphics => (
            "GS ( L",
            "Print the graphics data in the print buffer: Function 50".to_owned(),
            3,
        ),
        DecodedCommand::StoreRasterGraphics {
            extended_length,
            width_dots,
            height_dots,
        } => (
            if *extended_length { "GS 8 L" } else { "GS ( L" },
            format!(
                "Store the graphics data in the print buffer (raster format): Function 112 · {width_dots} × {height_dots} dots"
            ),
            3,
        ),
        DecodedCommand::SelectQrModel(model) => (
            "GS ( k",
            format!("QR Code: Set the model · Function 165 · model {model}"),
            3,
        ),
        DecodedCommand::SetQrModuleSize(size) => (
            "GS ( k",
            format!("QR Code: Set the size of module · Function 167 · {size} dots"),
            3,
        ),
        DecodedCommand::SelectQrErrorCorrection(level) => (
            "GS ( k",
            format!(
                "QR Code: Set the error correction level · Function 169 · level {}",
                match level {
                    0 => 'L',
                    1 => 'M',
                    2 => 'Q',
                    _ => 'H',
                }
            ),
            3,
        ),
        DecodedCommand::StoreQrData(bytes) => (
            "GS ( k",
            format!(
                "QR Code: Store the data in the symbol storage area · Function 180 · {bytes} {}",
                if *bytes == 1 { "byte" } else { "bytes" }
            ),
            3,
        ),
        DecodedCommand::QrCode(_) => (
            "GS ( k",
            "QR Code: Print the symbol data in the symbol storage area · Function 181".to_owned(),
            3,
        ),
        DecodedCommand::RasterImage {
            width_dots,
            height_dots,
            horizontal_scale,
            vertical_scale,
        } => (
            "GS v 0",
            format!(
                "Print raster bit image: {width_dots} × {height_dots} dots{}",
                magnification(*horizontal_scale, *vertical_scale)
            ),
            3,
        ),
        DecodedCommand::SkippedRasterImage => (
            "GS v 0",
            "Print raster bit image: skipped, the line already holds data".to_owned(),
            3,
        ),
        DecodedCommand::TextByte(byte) => (
            "Text",
            if byte.is_ascii_graphic() || *byte == b' ' {
                char::from(*byte).to_string()
            } else {
                format!("0x{byte:02X}")
            },
            0,
        ),
        DecodedCommand::Unknown(code) => {
            return CommandDisplay {
                name: unknown_name(*code),
                detail: "Unknown byte sequence".to_owned(),
                code_length: match code {
                    CommandCode::Control(_) => 1,
                    _ => 2,
                },
            };
        }
    };
    CommandDisplay {
        name: name.to_owned(),
        detail,
        code_length,
    }
}

/// Names the magnification of a raster image, which prints it larger.
fn magnification(horizontal_scale: u8, vertical_scale: u8) -> String {
    let mut scales = Vec::new();
    if horizontal_scale > 1 {
        scales.push("double-width");
    }
    if vertical_scale > 1 {
        scales.push("double-height");
    }
    if scales.is_empty() {
        return String::new();
    }
    format!(" · {}", scales.join(" · "))
}

fn switch(on: bool) -> &'static str {
    if on { "on" } else { "off" }
}

/// Lists the styles `ESC !` turns on, which Epson packs into one byte.
fn print_mode(mode: u8) -> String {
    let mut styles = vec![if mode & 0x01 == 0 { "Font A" } else { "Font B" }];
    for (bit, style) in [
        (0x08, "emphasized"),
        (0x10, "double-height"),
        (0x20, "double-width"),
        (0x80, "underline"),
    ] {
        if mode & bit != 0 {
            styles.push(style);
        }
    }
    styles.join(" · ")
}

fn bit_image_mode(mode: u8) -> &'static str {
    match mode {
        0 => "8-dot single-density",
        1 => "8-dot double-density",
        32 => "24-dot single-density",
        _ => "24-dot double-density",
    }
}

/// Names the fonts of `ESC M` and `GS f`, which share their parameter values.
fn font_name(font: u8) -> String {
    match font {
        0..=4 => format!("Font {}", char::from(b'A' + font)),
        97 => "Special font A".to_owned(),
        98 => "Special font B".to_owned(),
        font => format!("font {font}"),
    }
}

fn motion_unit(unit: u8) -> String {
    if unit == 0 {
        "the default".to_owned()
    } else {
        format!("1/{unit} inch")
    }
}

pub(super) fn international_character_set(set: u8) -> String {
    match set {
        0 => "U.S.A.",
        1 => "France",
        2 => "Germany",
        3 => "U.K.",
        4 => "Denmark I",
        5 => "Sweden",
        6 => "Italy",
        7 => "Spain I",
        8 => "Japan",
        9 => "Norway",
        10 => "Denmark II",
        11 => "Spain II",
        12 => "Latin America",
        13 => "Korea",
        14 => "Slovenia / Croatia",
        15 => "China",
        16 => "Vietnam",
        17 => "Arabia",
        set => return format!("set {set}"),
    }
    .to_owned()
}

/// Names a barcode system by the Function B value that selects it.
fn barcode_system(system: u8) -> String {
    match system {
        65 => "UPC-A",
        66 => "UPC-E",
        67 => "EAN-13",
        68 => "EAN-8",
        69 => "Code 39",
        70 => "ITF",
        71 => "Codabar",
        72 => "Code 93",
        73 => "Code 128",
        74 => "GS1-128",
        75 => "GS1 DataBar Omnidirectional",
        76 => "GS1 DataBar Truncated",
        77 => "GS1 DataBar Limited",
        78 => "GS1 DataBar Expanded",
        79 => "Code 128 auto",
        system => return format!("system {system}"),
    }
    .to_owned()
}

/// Shows barcode data as text where it reads as text, and as bytes otherwise.
fn readable(data: &[u8]) -> String {
    const SHOWN: usize = 32;
    match std::str::from_utf8(data) {
        Ok(text) if text.chars().all(|character| !character.is_control()) => {
            let shown = text.chars().take(SHOWN).collect::<String>();
            let ellipsis = if text.chars().count() > SHOWN {
                "…"
            } else {
                ""
            };
            format!("\"{shown}{ellipsis}\"")
        }
        _ => hexadecimal(&data[..data.len().min(SHOWN)]),
    }
}

fn unknown_name(code: CommandCode) -> String {
    match code {
        CommandCode::Control(opcode) => format!("Control {opcode:02X}"),
        CommandCode::Esc(opcode) => format!("ESC {}", opcode_name(opcode)),
        CommandCode::Gs(opcode) => format!("GS {}", opcode_name(opcode)),
    }
}

fn opcode_name(opcode: u8) -> String {
    if opcode.is_ascii_graphic() {
        char::from(opcode).to_string()
    } else {
        format!("{opcode:02X}")
    }
}

#[derive(Clone, Serialize)]
pub(crate) struct TextStyleResponse {
    font: &'static str,
    emphasized: bool,
    underline_thickness: u8,
    width_magnification: u8,
    height_magnification: u8,
    reversed: bool,
    justification: &'static str,
    code_page: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    encoding: Option<String>,
    international_character_set: String,
    right_side_character_spacing_dots: u32,
    line_spacing_dots: u32,
}

fn text_style_response(style: TextStyle) -> TextStyleResponse {
    TextStyleResponse {
        font: match style.font {
            TextFont::A => "A",
            TextFont::B => "B",
        },
        emphasized: style.emphasized,
        underline_thickness: style.underline_thickness,
        width_magnification: style.width_magnification,
        height_magnification: style.height_magnification,
        reversed: style.reversed,
        justification: justification_name(style.justification),
        code_page: style.code_page,
        encoding: style.encoding,
        international_character_set: international_character_set(style.international_character_set),
        right_side_character_spacing_dots: style.right_side_character_spacing_dots,
        line_spacing_dots: style.line_spacing_dots,
    }
}

#[derive(Clone, Serialize)]
struct AnnotationResponse {
    label: String,
    content: String,
}

#[derive(Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum EffectResponse {
    StateChange {
        state: &'static str,
        before: &'static str,
        after: &'static str,
    },
    Motion {
        before: PositionResponse,
        after: PositionResponse,
    },
    Paint {
        bounds: RegionResponse,
    },
}

#[derive(Clone, Serialize)]
struct PositionResponse {
    x: u32,
    y: u32,
}

#[derive(Clone, Serialize)]
struct RegionResponse {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

fn qr_annotation(data: &[u8]) -> AnnotationResponse {
    match std::str::from_utf8(data) {
        Ok(text) => AnnotationResponse {
            label: text.chars().flat_map(char::escape_default).collect(),
            content: text.to_owned(),
        },
        Err(_) => {
            let hexadecimal = data
                .iter()
                .map(|byte| format!("{byte:02X}"))
                .collect::<Vec<_>>()
                .join(" ");
            AnnotationResponse {
                label: hexadecimal.clone(),
                content: hexadecimal,
            }
        }
    }
}

fn effect_response(effect: Effect) -> EffectResponse {
    match effect {
        Effect::StateChange(StateChange::Justification { before, after }) => {
            EffectResponse::StateChange {
                state: "justification",
                before: justification_name(before),
                after: justification_name(after),
            }
        }
        Effect::Motion { before, after } => EffectResponse::Motion {
            before: PositionResponse {
                x: before.x,
                y: before.y,
            },
            after: PositionResponse {
                x: after.x,
                y: after.y,
            },
        },
        Effect::Paint { bounds } => EffectResponse::Paint {
            bounds: RegionResponse {
                x: bounds.x,
                y: bounds.y,
                width: bounds.width,
                height: bounds.height,
            },
        },
    }
}

fn justification_name(justification: Justification) -> &'static str {
    match justification {
        Justification::Left => "left",
        Justification::Center => "center",
        Justification::Right => "right",
    }
}

#[cfg(test)]
mod tests {
    use escpost_render::{
        CommandCode, CommandTrace, DecodedCommand, Justification, TextFont, TextStyle,
    };

    use super::{command_responses, display};

    fn described(command: DecodedCommand) -> (String, String) {
        let shown = display(&command);
        (shown.name, shown.detail)
    }

    #[test]
    fn control_codes_carry_their_epson_name() {
        assert_eq!(
            described(DecodedCommand::HorizontalTab),
            ("HT".to_owned(), "Horizontal tab".to_owned())
        );
        assert_eq!(
            described(DecodedCommand::LineFeed),
            ("LF".to_owned(), "Print and line feed".to_owned())
        );
        assert_eq!(
            described(DecodedCommand::CarriageReturn),
            ("CR".to_owned(), "Print and carriage return".to_owned())
        );
    }

    #[test]
    fn print_mode_lists_every_style_it_turns_on() {
        assert_eq!(
            described(DecodedCommand::SelectPrintMode(0x00)).1,
            "Set print mode(s): Font A"
        );
        assert_eq!(
            described(DecodedCommand::SelectPrintMode(0xb9)).1,
            "Set print mode(s): Font B · emphasized · double-height · double-width · underline"
        );
    }

    #[test]
    fn character_size_resolves_both_magnifications() {
        assert_eq!(
            described(DecodedCommand::SelectCharacterSize(0x00)).1,
            "Set character size: 1 × width · 1 × height"
        );
        assert_eq!(
            described(DecodedCommand::SelectCharacterSize(0x11)).1,
            "Set character size: 2 × width · 2 × height"
        );
    }

    #[test]
    fn a_raster_image_says_when_the_printer_magnifies_it() {
        assert_eq!(
            described(DecodedCommand::RasterImage {
                width_dots: 32,
                height_dots: 22,
                horizontal_scale: 1,
                vertical_scale: 1
            })
            .1,
            "Print raster bit image: 32 × 22 dots"
        );
        assert_eq!(
            described(DecodedCommand::RasterImage {
                width_dots: 32,
                height_dots: 22,
                horizontal_scale: 2,
                vertical_scale: 2
            })
            .1,
            "Print raster bit image: 32 × 22 dots · double-width · double-height"
        );
    }

    #[test]
    fn tab_positions_agree_in_number_with_the_columns_they_set() {
        assert_eq!(
            described(DecodedCommand::SetHorizontalTabPositions(vec![8])).1,
            "Set horizontal tab positions: column 8"
        );
        assert_eq!(
            described(DecodedCommand::SetHorizontalTabPositions(vec![8, 16])).1,
            "Set horizontal tab positions: columns 8, 16"
        );
        assert_eq!(
            described(DecodedCommand::SetHorizontalTabPositions(vec![])).1,
            "Set horizontal tab positions: cleared"
        );
    }

    #[test]
    fn a_command_says_whether_its_parameters_have_a_fixed_size() {
        let responses = command_responses(vec![
            CommandTrace {
                byte_range: 0..2,
                bytes: vec![0x1b, b'@'],
                style: None,
                command: DecodedCommand::Initialize,
                paint_lifecycle: None,
                effects: vec![],
            },
            CommandTrace {
                byte_range: 2..10,
                bytes: vec![0x1d, b'k', 73, 4, b'1', b'2', b'3', b'4'],
                style: None,
                command: DecodedCommand::PrintBarcode {
                    system: 73,
                    data: b"1234".to_vec(),
                },
                paint_lifecycle: None,
                effects: vec![],
            },
        ]);

        assert!(responses[0].fixed_parameters);
        assert!(!responses[1].fixed_parameters);
    }

    #[test]
    fn a_command_carries_the_style_it_left_behind() {
        let style = TextStyle {
            font: TextFont::B,
            emphasized: true,
            underline_thickness: 2,
            width_magnification: 2,
            height_magnification: 3,
            reversed: false,
            justification: Justification::Center,
            code_page: 2,
            encoding: Some("CP850".to_owned()),
            international_character_set: 2,
            right_side_character_spacing_dots: 3,
            line_spacing_dots: 30,
        };
        let responses = command_responses(vec![
            CommandTrace {
                byte_range: 0..3,
                bytes: vec![0x1b, b'!', 0x39],
                style: Some(style),
                command: DecodedCommand::SelectPrintMode(0x39),
                paint_lifecycle: None,
                effects: vec![],
            },
            CommandTrace {
                byte_range: 3..4,
                bytes: vec![b'A'],
                style: None,
                command: DecodedCommand::TextByte(b'A'),
                paint_lifecycle: None,
                effects: vec![],
            },
        ]);

        let shown = responses[0]
            .text_style
            .as_ref()
            .expect("the style rides along");
        assert_eq!(shown.font, "B");
        assert!(shown.emphasized);
        assert_eq!(shown.underline_thickness, 2);
        assert_eq!(shown.width_magnification, 2);
        assert_eq!(shown.height_magnification, 3);
        assert_eq!(shown.justification, "center");
        assert_eq!(shown.encoding.as_deref(), Some("CP850"));
        assert_eq!(shown.international_character_set, "Germany");
        assert!(responses[1].text_style.is_none());
    }

    #[test]
    fn a_cut_says_its_shape_and_any_feed_before_it() {
        assert_eq!(
            described(DecodedCommand::CutPaper {
                full: true,
                feed: None
            }),
            (
                "GS V".to_owned(),
                "Set cut mode and cut paper: full cut".to_owned()
            )
        );
        assert_eq!(
            described(DecodedCommand::CutPaper {
                full: false,
                feed: Some(24)
            })
            .1,
            "Set cut mode and cut paper: partial cut · feeds 24 × vertical motion unit first"
        );
    }

    #[test]
    fn a_barcode_names_its_system_and_shows_its_data() {
        assert_eq!(
            described(DecodedCommand::PrintBarcode {
                system: 73,
                data: b"{B1234".to_vec()
            }),
            (
                "GS k".to_owned(),
                "Print barcode: Code 128 · \"{B1234\"".to_owned()
            )
        );
    }

    #[test]
    fn a_code_table_prefers_the_encoding_the_profile_names() {
        assert_eq!(
            described(DecodedCommand::SelectCodeTable {
                table: 2,
                encoding: Some("CP850".to_owned())
            })
            .1,
            "Set character code table: CP850"
        );
        assert_eq!(
            described(DecodedCommand::SelectCodeTable {
                table: 2,
                encoding: None
            })
            .1,
            "Set character code table: page 2"
        );
    }

    #[test]
    fn qr_functions_keep_the_function_number_of_the_manual() {
        assert_eq!(
            described(DecodedCommand::SelectQrErrorCorrection(1)),
            (
                "GS ( k".to_owned(),
                "QR Code: Set the error correction level · Function 169 · level M".to_owned()
            )
        );
        assert_eq!(
            described(DecodedCommand::QrCode(b"hello".to_vec())).1,
            "QR Code: Print the symbol data in the symbol storage area · Function 181"
        );
    }

    #[test]
    fn graphics_storage_names_the_command_that_carried_it() {
        assert_eq!(
            described(DecodedCommand::StoreRasterGraphics {
                extended_length: false,
                width_dots: 576,
                height_dots: 128
            }),
            (
                "GS ( L".to_owned(),
                "Store the graphics data in the print buffer (raster format): Function 112 · 576 × 128 dots".to_owned()
            )
        );
        assert_eq!(
            described(DecodedCommand::StoreRasterGraphics {
                extended_length: true,
                width_dots: 576,
                height_dots: 128
            })
            .0,
            "GS 8 L"
        );
    }

    #[test]
    fn an_unlisted_command_is_named_as_an_unknown_sequence() {
        assert_eq!(
            described(DecodedCommand::Unknown(CommandCode::Esc(b'5'))),
            ("ESC 5".to_owned(), "Unknown byte sequence".to_owned())
        );
        assert_eq!(
            described(DecodedCommand::Unknown(CommandCode::Control(0x1f))).0,
            "Control 1F"
        );
    }

    #[test]
    fn a_response_splits_the_command_bytes_from_its_parameters() {
        let responses = command_responses(vec![CommandTrace {
            byte_range: 0..3,
            bytes: vec![0x1b, b'a', 1],
            style: None,
            command: DecodedCommand::SetJustification(Justification::Center),
            paint_lifecycle: None,
            effects: vec![],
        }]);

        assert_eq!(responses[0].code_bytes, "1B 61");
        assert_eq!(responses[0].capped_parameter_bytes, "01");
        assert_eq!(responses[0].total_parameter_bytes, 1);
    }

    #[test]
    fn a_text_byte_is_all_parameter_and_no_command() {
        let responses = command_responses(vec![CommandTrace {
            byte_range: 4..5,
            bytes: vec![b'A'],
            style: None,
            command: DecodedCommand::TextByte(b'A'),
            paint_lifecycle: None,
            effects: vec![],
        }]);

        assert_eq!(responses[0].code_bytes, "");
        assert_eq!(responses[0].capped_parameter_bytes, "41");
        assert_eq!(responses[0].total_parameter_bytes, 1);
    }

    #[test]
    fn a_long_payload_shows_its_start_and_counts_the_rest() {
        let mut bytes = vec![0x1d, b'v', b'0'];
        bytes.extend(std::iter::repeat_n(0x5a, 30));

        let responses = command_responses(vec![CommandTrace {
            byte_range: 0..2048,
            bytes,
            style: None,
            command: DecodedCommand::RasterImage {
                width_dots: 16,
                height_dots: 128,
                horizontal_scale: 1,
                vertical_scale: 1,
            },
            paint_lifecycle: None,
            effects: vec![],
        }]);

        assert_eq!(responses[0].code_bytes, "1D 76 30");
        assert_eq!(
            responses[0].capped_parameter_bytes,
            "5A 5A 5A 5A 5A 5A 5A 5A"
        );
        assert_eq!(responses[0].total_parameter_bytes, 2045);
    }
}
