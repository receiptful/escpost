//! ESC/POS command parsing and dispatch.

use crate::state::{BufferedGraphics, HriPosition, Justification, PrinterState};
use crate::surface::RenderSurface;
use crate::trace::{CommandSink, DecodedCommand, Effect, StateChange};
use crate::{RenderError, barcode, qr};
use escpost_profiles::BarcodeSystem;

const MAX_QR_STORE_PARAMETER_BYTES: usize = 7092;

pub(crate) fn execute_esc_command<S: RenderSurface, C: CommandSink>(
    data: &[u8],
    offset: usize,
    state: &mut PrinterState<S>,
    command_sink: &mut C,
) -> Result<usize, RenderError> {
    let Some(command) = data.get(1).copied() else {
        return Err(RenderError::TruncatedCommand {
            command: "ESC",
            offset,
        });
    };

    match command {
        0x40 => {
            state.initialize();
            Ok(2)
        }
        0x20 => {
            let Some(spacing) = data.get(2).copied() else {
                return Err(RenderError::TruncatedCommand {
                    command: "ESC SP",
                    offset,
                });
            };
            state.set_right_side_character_spacing(spacing);
            Ok(3)
        }
        0x24 => {
            let Some((&low, &high)) = data.get(2).zip(data.get(3)) else {
                return Err(RenderError::TruncatedCommand {
                    command: "ESC $",
                    offset,
                });
            };
            state.set_absolute_print_position(u16::from_le_bytes([low, high]));
            Ok(4)
        }
        0x21 => {
            let Some(mode) = data.get(2).copied() else {
                return Err(RenderError::TruncatedCommand {
                    command: "ESC !",
                    offset,
                });
            };
            state.set_print_mode(mode);
            Ok(3)
        }
        0x2a => execute_esc_star(data, offset, state),
        0x2d => {
            let Some(mode) = data.get(2).copied() else {
                return Err(RenderError::TruncatedCommand {
                    command: "ESC -",
                    offset,
                });
            };
            let thickness = match mode {
                0 | 48 => 0,
                1 | 49 => 1,
                2 | 50 => 2,
                mode => return Err(RenderError::UnsupportedUnderlineMode { mode, offset }),
            };
            state.set_underline(thickness);
            Ok(3)
        }
        0x32 => {
            state.restore_default_line_spacing();
            Ok(2)
        }
        0x33 => {
            let Some(spacing) = data.get(2).copied() else {
                return Err(RenderError::TruncatedCommand {
                    command: "ESC 3",
                    offset,
                });
            };
            state.set_line_spacing(spacing);
            Ok(3)
        }
        0x44 => execute_esc_d(data, offset, state),
        0x45 => {
            let Some(emphasis) = data.get(2).copied() else {
                return Err(RenderError::TruncatedCommand {
                    command: "ESC E",
                    offset,
                });
            };
            state.set_emphasis(emphasis & 0x01 != 0);
            Ok(3)
        }
        0x4a => {
            let Some(distance) = data.get(2).copied() else {
                return Err(RenderError::TruncatedCommand {
                    command: "ESC J",
                    offset,
                });
            };
            state.execute_esc_j(distance)?;
            Ok(3)
        }
        0x4d => {
            let Some(font) = data.get(2).copied() else {
                return Err(RenderError::TruncatedCommand {
                    command: "ESC M",
                    offset,
                });
            };
            match font {
                0 | 48 => state.select_font_a(),
                1 | 49 => state.select_font_b(),
                font => return Err(RenderError::UnsupportedCharacterFont { font, offset }),
            }
            Ok(3)
        }
        0x52 => {
            let Some(character_set) = data.get(2).copied() else {
                return Err(RenderError::TruncatedCommand {
                    command: "ESC R",
                    offset,
                });
            };
            if character_set > 17 {
                return Err(RenderError::UnsupportedInternationalCharacterSet {
                    character_set,
                    offset,
                });
            }
            state.select_international_character_set(character_set);
            Ok(3)
        }
        0x5c => {
            let Some((&low, &high)) = data.get(2).zip(data.get(3)) else {
                return Err(RenderError::TruncatedCommand {
                    command: "ESC \\",
                    offset,
                });
            };
            state.set_relative_print_position(i16::from_le_bytes([low, high]));
            Ok(4)
        }
        0x61 => {
            let Some(justification) = data.get(2).copied() else {
                return Err(RenderError::TruncatedCommand {
                    command: "ESC a",
                    offset,
                });
            };
            let justification = match justification {
                0 | 48 => Justification::Left,
                1 | 49 => Justification::Center,
                2 | 50 => Justification::Right,
                justification => {
                    return Err(RenderError::UnsupportedJustification {
                        justification,
                        offset,
                    });
                }
            };
            if C::ENABLED {
                let before = state.trace_justification();
                state.set_justification(justification);
                let after = state.trace_justification();
                command_sink.describe_command(
                    DecodedCommand::SetJustification(justification.into()),
                    (before != after)
                        .then(|| {
                            Effect::StateChange(StateChange::Justification {
                                before: before.into(),
                                after: after.into(),
                            })
                        })
                        .into_iter()
                        .collect(),
                );
            } else {
                state.set_justification(justification);
            }
            Ok(3)
        }
        0x64 => {
            let Some(lines) = data.get(2).copied() else {
                return Err(RenderError::TruncatedCommand {
                    command: "ESC d",
                    offset,
                });
            };
            state.feed_lines(lines)?;
            Ok(3)
        }
        0x70 => {
            let Some((&connector, timing)) = data.get(2).zip(data.get(3..5)) else {
                return Err(RenderError::TruncatedCommand {
                    command: "ESC p",
                    offset,
                });
            };
            state.drawer_pulse(connector, timing[0], timing[1], offset)?;
            Ok(5)
        }
        0x74 => {
            let Some(code_page) = data.get(2).copied() else {
                return Err(RenderError::TruncatedCommand {
                    command: "ESC t",
                    offset,
                });
            };
            if state.code_page_encoding(code_page).is_none() {
                return Err(RenderError::UnsupportedCodePage {
                    code_page,
                    encoding: "<not present in printer profile>".to_owned(),
                    offset,
                });
            }

            // Every ESC/POS character table shares printable ASCII. Remember a
            // known table even when its extended or multibyte range is outside
            // v1 so later ASCII can still be rendered faithfully.
            if C::ENABLED {
                let encoding = state.code_page_encoding(code_page).map(str::to_owned);
                command_sink.describe_command(
                    DecodedCommand::SelectCodeTable {
                        table: code_page,
                        encoding,
                    },
                    vec![],
                );
            }
            state.select_code_page(code_page);
            Ok(3)
        }
        command => Err(RenderError::UnsupportedEscCommand { command, offset }),
    }
}

fn execute_esc_d<S: RenderSurface>(
    data: &[u8],
    offset: usize,
    state: &mut PrinterState<S>,
) -> Result<usize, RenderError> {
    let mut columns = Vec::new();
    let mut command_length = 2;

    loop {
        let Some(column) = data.get(command_length).copied() else {
            return Err(RenderError::TruncatedCommand {
                command: "ESC D",
                offset,
            });
        };
        if column == 0 {
            command_length += 1;
            break;
        }

        let no_longer_ascending = columns.last().is_some_and(|&previous| column <= previous);
        if columns.len() == 32 || no_longer_ascending {
            // Epson treats the first excess or non-ascending byte as normal
            // input, so leave it for the outer parser instead of consuming it.
            break;
        }

        columns.push(column);
        command_length += 1;
    }

    state.set_tab_positions(&columns);
    Ok(command_length)
}

fn execute_esc_star<S: RenderSurface>(
    data: &[u8],
    offset: usize,
    state: &mut PrinterState<S>,
) -> Result<usize, RenderError> {
    if data.len() < 5 {
        return Err(RenderError::TruncatedCommand {
            command: "ESC *",
            offset,
        });
    }
    state.require_column_bit_image(offset)?;

    let mode = data[2];
    let (bytes_per_column, horizontal_scale, vertical_pitch) = match mode {
        0 => (1, 2, state.esc_star_8_dot_vertical_pitch),
        1 => (1, 1, state.esc_star_8_dot_vertical_pitch),
        32 => (3, 2, 1),
        33 => (3, 1, 1),
        mode => {
            return Err(RenderError::UnsupportedBitImageMode { mode, offset });
        }
    };
    let columns = usize::from(data[3]) + usize::from(data[4]) * 256;
    let payload_length = columns.saturating_mul(bytes_per_column);
    state.validate_command_payload_size(payload_length)?;
    let command_length = 5 + payload_length;
    let Some(payload) = data.get(5..command_length) else {
        return Err(RenderError::TruncatedCommand {
            command: "ESC *",
            offset,
        });
    };

    state.paint_bit_image(payload, bytes_per_column, horizontal_scale, vertical_pitch);
    Ok(command_length)
}

pub(crate) fn execute_gs_command<S: RenderSurface, C: CommandSink>(
    data: &[u8],
    offset: usize,
    state: &mut PrinterState<S>,
    command_sink: &mut C,
) -> Result<usize, RenderError> {
    let Some(command) = data.get(1).copied() else {
        return Err(RenderError::TruncatedCommand {
            command: "GS",
            offset,
        });
    };

    match command {
        0x21 => {
            let Some(size) = data.get(2).copied() else {
                return Err(RenderError::TruncatedCommand {
                    command: "GS !",
                    offset,
                });
            };
            state.set_character_size(size);
            Ok(3)
        }
        0x28 => match data.get(2) {
            Some(b'L') => execute_gs_parenthesized_l(data, offset, state),
            Some(b'k') => execute_gs_parenthesized_k(data, offset, state, command_sink),
            Some(_) => Err(RenderError::UnsupportedGsCommand {
                command: 0x28,
                offset,
            }),
            None => Err(RenderError::TruncatedCommand {
                command: "GS (",
                offset,
            }),
        },
        0x38 => execute_gs_8_l(data, offset, state),
        0x42 => {
            let Some(reverse) = data.get(2).copied() else {
                return Err(RenderError::TruncatedCommand {
                    command: "GS B",
                    offset,
                });
            };
            state.set_reverse(reverse & 0x01 != 0);
            Ok(3)
        }
        0x48 => {
            let Some(position) = data.get(2).copied() else {
                return Err(RenderError::TruncatedCommand {
                    command: "GS H",
                    offset,
                });
            };
            let position = match position {
                0 | 48 => HriPosition::None,
                1 | 49 => HriPosition::Above,
                2 | 50 => HriPosition::Below,
                3 | 51 => HriPosition::AboveAndBelow,
                value => {
                    return Err(RenderError::InvalidBarcodeParameter {
                        parameter: "hri_position",
                        value,
                        offset,
                    });
                }
            };
            state.set_hri_position(position);
            Ok(3)
        }
        0x4c => {
            let Some((&low, &high)) = data.get(2).zip(data.get(3)) else {
                return Err(RenderError::TruncatedCommand {
                    command: "GS L",
                    offset,
                });
            };
            state.set_left_margin(u16::from_le_bytes([low, high]));
            Ok(4)
        }
        0x50 => {
            let Some((&horizontal, &vertical)) = data.get(2).zip(data.get(3)) else {
                return Err(RenderError::TruncatedCommand {
                    command: "GS P",
                    offset,
                });
            };
            state.set_motion_units(horizontal, vertical);
            Ok(4)
        }
        0x57 => {
            let Some((&low, &high)) = data.get(2).zip(data.get(3)) else {
                return Err(RenderError::TruncatedCommand {
                    command: "GS W",
                    offset,
                });
            };
            state.set_print_area_width(u16::from_le_bytes([low, high]));
            Ok(4)
        }
        0x66 => {
            let Some(font) = data.get(2).copied() else {
                return Err(RenderError::TruncatedCommand {
                    command: "GS f",
                    offset,
                });
            };
            match font {
                0 | 48 => state.select_hri_font_a(),
                1 | 49 => state.select_hri_font_b(),
                value => {
                    return Err(RenderError::InvalidBarcodeParameter {
                        parameter: "hri_font",
                        value,
                        offset,
                    });
                }
            }
            Ok(3)
        }
        0x68 => {
            let Some(height) = data.get(2).copied() else {
                return Err(RenderError::TruncatedCommand {
                    command: "GS h",
                    offset,
                });
            };
            if height == 0 {
                return Err(RenderError::InvalidBarcodeParameter {
                    parameter: "height",
                    value: height,
                    offset,
                });
            }
            state.set_barcode_height(height);
            Ok(3)
        }
        0x6b => execute_gs_k(data, offset, state),
        0x56 => execute_gs_v(data, offset, state),
        0x76 => execute_gs_v0(data, offset, state),
        0x77 => {
            let Some(width) = data.get(2).copied() else {
                return Err(RenderError::TruncatedCommand {
                    command: "GS w",
                    offset,
                });
            };
            if !(2..=6).contains(&width) {
                return Err(RenderError::InvalidBarcodeParameter {
                    parameter: "module_width",
                    value: width,
                    offset,
                });
            }
            state.set_barcode_module_width(width);
            Ok(3)
        }
        command => Err(RenderError::UnsupportedGsCommand { command, offset }),
    }
}

fn execute_gs_k<S: RenderSurface>(
    data: &[u8],
    offset: usize,
    state: &mut PrinterState<S>,
) -> Result<usize, RenderError> {
    let Some(system) = data.get(2).copied() else {
        return Err(RenderError::TruncatedCommand {
            command: "GS k",
            offset,
        });
    };

    let (system, payload, mut command_length, is_function_a) = match system {
        function_a @ 0..=6 => {
            let barcode_system = barcode_system_from_function_b(function_a + 65)
                .expect("every Function A command maps to a known barcode system");
            state.require_barcode_system(barcode_system, true, offset)?;
            let (payload, command_length) = function_a_barcode_payload(data, function_a, offset)?;
            state.validate_command_payload_size(payload.len())?;
            (function_a + 65, payload, command_length, true)
        }
        function_b @ 65..=79 => {
            let barcode_system = barcode_system_from_function_b(function_b)
                .expect("the accepted Function B range contains known barcode systems");
            state.require_barcode_system(barcode_system, false, offset)?;
            let payload_length =
                usize::from(*data.get(3).ok_or(RenderError::TruncatedCommand {
                    command: "GS k",
                    offset,
                })?);
            state.validate_command_payload_size(payload_length)?;
            let command_length = 4usize.saturating_add(payload_length);
            let payload = data
                .get(4..command_length)
                .ok_or(RenderError::TruncatedCommand {
                    command: "GS k",
                    offset,
                })?;
            (function_b, payload, command_length, false)
        }
        _ => {
            return Err(RenderError::InvalidBarcodeData {
                system: "unknown",
                offset,
                reason: "barcode system is not supported",
            });
        }
    };

    // Function A predates the explicit byte count. Epson documents that its
    // ITF mode drops the final digit when the NUL-terminated count is odd.
    let payload = if is_function_a && system == 70 && !payload.len().is_multiple_of(2) {
        &payload[..payload.len() - 1]
    } else {
        payload
    };
    let payload = if !is_function_a && system == 69 {
        let first_possible_stop = usize::from(payload.first() == Some(&b'*'));
        let stop = payload[first_possible_stop..]
            .iter()
            .position(|character| *character == b'*')
            .map(|position| first_possible_stop + position);
        if let Some(stop) = stop.filter(|stop| stop + 1 < payload.len()) {
            // In Function B the declared byte count does not swallow bytes
            // after a Code 39 stop. The parser must return them to the main
            // ESC/POS stream as ordinary input.
            command_length = 4 + stop + 1;
            &payload[..=stop]
        } else {
            payload
        }
    } else {
        payload
    };

    let barcode = match system {
        65 => barcode::encode_upca(payload).map_err(|error| RenderError::InvalidBarcodeData {
            system: "UPC-A",
            offset,
            reason: match error {
                barcode::BarcodeError::Length => "expected 11 or 12 digits",
                barcode::BarcodeError::Character => "expected decimal digits only",
                barcode::BarcodeError::Format => "invalid UPC-A data format",
            },
        })?,
        66 => barcode::encode_upce(payload).map_err(|error| RenderError::InvalidBarcodeData {
            system: "UPC-E",
            offset,
            reason: match error {
                barcode::BarcodeError::Length => "expected 6, 7, 8, 11, or 12 digits",
                barcode::BarcodeError::Character => "expected decimal digits only",
                barcode::BarcodeError::Format => {
                    "expected number system 0 and a compressible UPC-A value"
                }
            },
        })?,
        67 => barcode::encode_ean13(payload).map_err(|error| RenderError::InvalidBarcodeData {
            system: "EAN-13",
            offset,
            reason: match error {
                barcode::BarcodeError::Length => "expected 12 or 13 digits",
                barcode::BarcodeError::Character => "expected decimal digits only",
                barcode::BarcodeError::Format => "invalid EAN-13 data format",
            },
        })?,
        68 => barcode::encode_ean8(payload).map_err(|error| RenderError::InvalidBarcodeData {
            system: "EAN-8",
            offset,
            reason: match error {
                barcode::BarcodeError::Length => "expected 7 or 8 digits",
                barcode::BarcodeError::Character => "expected decimal digits only",
                barcode::BarcodeError::Format => "invalid EAN-8 data format",
            },
        })?,
        69 => barcode::encode_code39(payload).map_err(|error| RenderError::InvalidBarcodeData {
            system: "Code 39",
            offset,
            reason: match error {
                barcode::BarcodeError::Length => "expected at least one character",
                barcode::BarcodeError::Character => "contains an unsupported character",
                barcode::BarcodeError::Format => "the stop character may appear only at the end",
            },
        })?,
        70 => barcode::encode_itf(payload).map_err(|error| RenderError::InvalidBarcodeData {
            system: "ITF",
            offset,
            reason: match error {
                barcode::BarcodeError::Length => "expected an even number of at least two digits",
                barcode::BarcodeError::Character => "expected decimal digits only",
                barcode::BarcodeError::Format => "invalid ITF data format",
            },
        })?,
        71 => {
            barcode::encode_codabar(payload).map_err(|error| RenderError::InvalidBarcodeData {
                system: "Codabar",
                offset,
                reason: match error {
                    barcode::BarcodeError::Length => "expected start and stop characters",
                    barcode::BarcodeError::Character => "contains an unsupported character",
                    barcode::BarcodeError::Format => {
                        "expected A through D start and stop characters"
                    }
                },
            })?
        }
        72 => barcode::encode_code93(payload).map_err(|error| RenderError::InvalidBarcodeData {
            system: "Code 93",
            offset,
            reason: match error {
                barcode::BarcodeError::Length => "expected at least one character",
                barcode::BarcodeError::Character => "expected bytes 00h through 7Fh",
                barcode::BarcodeError::Format => "invalid Code 93 data format",
            },
        })?,
        73 => {
            barcode::encode_code128(payload).map_err(|error| RenderError::InvalidBarcodeData {
                system: "Code 128",
                offset,
                reason: match error {
                    barcode::BarcodeError::Length => {
                        "expected an explicit {A, {B, or {C start sequence"
                    }
                    barcode::BarcodeError::Character => {
                        "character is not valid in the selected code set"
                    }
                    barcode::BarcodeError::Format => "invalid Code 128 code-set data",
                },
            })?
        }
        74 => {
            barcode::encode_gs1_128(payload).map_err(|error| RenderError::InvalidBarcodeData {
                system: "GS1-128",
                offset,
                reason: match error {
                    barcode::BarcodeError::Length => "expected 2 through 255 bytes",
                    barcode::BarcodeError::Character => "expected bytes 00h through 7Fh",
                    barcode::BarcodeError::Format => "invalid GS1-128 data structure",
                },
            })?
        }
        75 => barcode::encode_gs1_databar_omnidirectional(payload).map_err(|error| {
            RenderError::InvalidBarcodeData {
                system: "GS1 DataBar Omnidirectional",
                offset,
                reason: match error {
                    barcode::BarcodeError::Length => "expected exactly 13 digits",
                    barcode::BarcodeError::Character => "expected decimal digits only",
                    barcode::BarcodeError::Format => {
                        "could not encode the GS1 DataBar Omnidirectional value"
                    }
                },
            }
        })?,
        76 => barcode::encode_gs1_databar_truncated(payload).map_err(|error| {
            RenderError::InvalidBarcodeData {
                system: "GS1 DataBar Truncated",
                offset,
                reason: match error {
                    barcode::BarcodeError::Length => "expected exactly 13 digits",
                    barcode::BarcodeError::Character => "expected decimal digits only",
                    barcode::BarcodeError::Format => {
                        "could not encode the GS1 DataBar Truncated value"
                    }
                },
            }
        })?,
        77 => barcode::encode_gs1_databar_limited(payload).map_err(|error| {
            RenderError::InvalidBarcodeData {
                system: "GS1 DataBar Limited",
                offset,
                reason: match error {
                    barcode::BarcodeError::Length => "expected exactly 13 digits",
                    barcode::BarcodeError::Character => "expected decimal digits only",
                    barcode::BarcodeError::Format => {
                        "expected a value between 0000000000000 and 1999999999999"
                    }
                },
            }
        })?,
        78 => barcode::encode_gs1_databar_expanded(payload).map_err(|error| {
            RenderError::InvalidBarcodeData {
                system: "GS1 DataBar Expanded",
                offset,
                reason: match error {
                    barcode::BarcodeError::Length => "expected at least two bytes",
                    barcode::BarcodeError::Character => {
                        "contains a character outside the GS1 encodable set"
                    }
                    barcode::BarcodeError::Format => "invalid GS1 DataBar Expanded data structure",
                },
            }
        })?,
        79 => barcode::encode_code128_auto(payload).map_err(|error| {
            RenderError::InvalidBarcodeData {
                system: "Code 128 auto",
                offset,
                reason: match error {
                    barcode::BarcodeError::Length => "expected at least one byte",
                    barcode::BarcodeError::Character => "could not encode byte in Code 128",
                    barcode::BarcodeError::Format => "could not plan automatic Code 128 data",
                },
            }
        })?,
        _ => {
            return Err(RenderError::InvalidBarcodeData {
                system: "unknown",
                offset,
                reason: "barcode system is not implemented yet",
            });
        }
    };
    state.print_barcode(&barcode, offset)?;
    Ok(command_length)
}

fn barcode_system_from_function_b(system: u8) -> Option<BarcodeSystem> {
    Some(match system {
        65 => BarcodeSystem::UpcA,
        66 => BarcodeSystem::UpcE,
        67 => BarcodeSystem::Ean13,
        68 => BarcodeSystem::Ean8,
        69 => BarcodeSystem::Code39,
        70 => BarcodeSystem::Itf,
        71 => BarcodeSystem::Codabar,
        72 => BarcodeSystem::Code93,
        73 => BarcodeSystem::Code128,
        74 => BarcodeSystem::Gs1_128,
        75 => BarcodeSystem::Gs1DataBarOmnidirectional,
        76 => BarcodeSystem::Gs1DataBarTruncated,
        77 => BarcodeSystem::Gs1DataBarLimited,
        78 => BarcodeSystem::Gs1DataBarExpanded,
        79 => BarcodeSystem::Code128Auto,
        _ => return None,
    })
}

fn function_a_barcode_payload(
    data: &[u8],
    system: u8,
    offset: usize,
) -> Result<(&[u8], usize), RenderError> {
    let remaining = data.get(3..).ok_or(RenderError::TruncatedCommand {
        command: "GS k",
        offset,
    })?;
    let nul = remaining.iter().position(|byte| *byte == 0);

    if system == 4 {
        // A leading '*' is Code 39's start character. A later '*' is the stop
        // character and ends command processing immediately, even before the
        // NUL that normally frames Function A.
        let first_possible_stop = usize::from(remaining.first() == Some(&b'*'));
        let stop = remaining[first_possible_stop..]
            .iter()
            .position(|character| *character == b'*')
            .map(|position| first_possible_stop + position);
        if let Some(stop) = stop.filter(|stop| nul.is_none_or(|nul| *stop < nul)) {
            let payload_length = stop + 1;
            return Ok((&remaining[..payload_length], 3 + payload_length));
        }
    }

    let payload_length = nul.ok_or(RenderError::TruncatedCommand {
        command: "GS k",
        offset,
    })?;
    Ok((
        &remaining[..payload_length],
        4usize.saturating_add(payload_length),
    ))
}

fn execute_gs_parenthesized_l<S: RenderSurface>(
    data: &[u8],
    offset: usize,
    state: &mut PrinterState<S>,
) -> Result<usize, RenderError> {
    if data.len() < 5 || data[2] != b'L' {
        return Err(RenderError::TruncatedCommand {
            command: "GS ( L",
            offset,
        });
    }

    let parameter_length = usize::from(u16::from_le_bytes([data[3], data[4]]));
    state.validate_command_payload_size(parameter_length)?;
    let command_length = 5 + parameter_length;
    let Some(parameters) = data.get(5..command_length) else {
        return Err(RenderError::TruncatedCommand {
            command: "GS ( L",
            offset,
        });
    };
    execute_graphics_function(parameters, false, offset, state)?;
    Ok(command_length)
}

fn execute_gs_parenthesized_k<S: RenderSurface, C: CommandSink>(
    data: &[u8],
    offset: usize,
    state: &mut PrinterState<S>,
    command_sink: &mut C,
) -> Result<usize, RenderError> {
    if data.len() < 5 || data[2] != b'k' {
        return Err(RenderError::TruncatedCommand {
            command: "GS ( k",
            offset,
        });
    }

    let parameter_length = usize::from(u16::from_le_bytes([data[3], data[4]]));
    state.validate_command_payload_size(parameter_length)?;
    let command_length = 5usize.saturating_add(parameter_length);
    let Some(parameters) = data.get(5..command_length) else {
        return Err(RenderError::TruncatedCommand {
            command: "GS ( k",
            offset,
        });
    };
    let prints_qr = parameters.starts_with(&[49, 81]);
    execute_qr_function(parameters, offset, state)?;
    if C::ENABLED && prints_qr {
        command_sink.describe_command(
            DecodedCommand::QrCode(state.trace_qr_data().to_vec()),
            vec![],
        );
    }
    Ok(command_length)
}

fn execute_qr_function<S: RenderSurface>(
    parameters: &[u8],
    offset: usize,
    state: &mut PrinterState<S>,
) -> Result<(), RenderError> {
    let Some((&code_type, &function)) = parameters.first().zip(parameters.get(1)) else {
        return Err(RenderError::TruncatedCommand {
            command: "GS ( k",
            offset,
        });
    };
    if code_type != 49 {
        return Err(RenderError::InvalidQrParameter {
            parameter: "cn",
            value: code_type,
            offset,
        });
    }

    match function {
        65 => {
            let Some((&model, &reserved)) = parameters.get(2).zip(parameters.get(3)) else {
                return Err(RenderError::TruncatedCommand {
                    command: "GS ( k Function 165",
                    offset,
                });
            };
            if parameters.len() != 4 || reserved != 0 {
                return Err(RenderError::InvalidQrParameter {
                    parameter: "n2",
                    value: reserved,
                    offset,
                });
            }
            if model != 50 {
                return Err(RenderError::UnsupportedQrModel { model, offset });
            }
            state.select_qr_model_2(offset)
        }
        67 => {
            let Some(&module_size) = parameters.get(2) else {
                return Err(RenderError::TruncatedCommand {
                    command: "GS ( k Function 167",
                    offset,
                });
            };
            if parameters.len() != 3 || !(1..=16).contains(&module_size) {
                return Err(RenderError::InvalidQrParameter {
                    parameter: "module_size",
                    value: module_size,
                    offset,
                });
            }
            state.set_qr_module_size(module_size, offset)
        }
        69 => {
            let Some(&level) = parameters.get(2) else {
                return Err(RenderError::TruncatedCommand {
                    command: "GS ( k Function 169",
                    offset,
                });
            };
            if parameters.len() != 3 {
                return Err(RenderError::InvalidQrParameter {
                    parameter: "error_correction",
                    value: level,
                    offset,
                });
            }
            let level = match level {
                48 => qr::ErrorCorrection::Low,
                49 => qr::ErrorCorrection::Medium,
                50 => qr::ErrorCorrection::Quartile,
                51 => qr::ErrorCorrection::High,
                value => {
                    return Err(RenderError::InvalidQrParameter {
                        parameter: "error_correction",
                        value,
                        offset,
                    });
                }
            };
            state.set_qr_error_correction(level, offset)
        }
        80 => {
            if parameters.len() > MAX_QR_STORE_PARAMETER_BYTES {
                return Err(RenderError::InvalidQrData {
                    offset,
                    reason: "store command exceeds the 7092-byte parameter limit",
                });
            }
            let Some((&mode, data)) = parameters.get(2).zip(parameters.get(3..)) else {
                return Err(RenderError::TruncatedCommand {
                    command: "GS ( k Function 180",
                    offset,
                });
            };
            if mode != 48 {
                return Err(RenderError::InvalidQrParameter {
                    parameter: "m",
                    value: mode,
                    offset,
                });
            }
            state.store_qr_data(data, offset)
        }
        81 => {
            let Some(&mode) = parameters.get(2) else {
                return Err(RenderError::TruncatedCommand {
                    command: "GS ( k Function 181",
                    offset,
                });
            };
            if parameters.len() != 3 || mode != 48 {
                return Err(RenderError::InvalidQrParameter {
                    parameter: "m",
                    value: mode,
                    offset,
                });
            }
            state.print_qr(offset)
        }
        function => Err(RenderError::UnsupportedQrFunction { function, offset }),
    }
}

fn execute_gs_8_l<S: RenderSurface>(
    data: &[u8],
    offset: usize,
    state: &mut PrinterState<S>,
) -> Result<usize, RenderError> {
    if data.len() < 7 || data[2] != b'L' {
        return Err(RenderError::TruncatedCommand {
            command: "GS 8 L",
            offset,
        });
    }

    let parameter_length = u32::from_le_bytes([data[3], data[4], data[5], data[6]]) as usize;
    state.validate_command_payload_size(parameter_length)?;
    let command_length = 7usize.saturating_add(parameter_length);
    let Some(parameters) = data.get(7..command_length) else {
        return Err(RenderError::TruncatedCommand {
            command: "GS 8 L",
            offset,
        });
    };
    execute_graphics_function(parameters, true, offset, state)?;
    Ok(command_length)
}

fn execute_graphics_function<S: RenderSurface>(
    parameters: &[u8],
    extended_length: bool,
    offset: usize,
    state: &mut PrinterState<S>,
) -> Result<(), RenderError> {
    let Some((&mode, &function)) = parameters.first().zip(parameters.get(1)) else {
        return Err(RenderError::TruncatedCommand {
            command: if extended_length { "GS 8 L" } else { "GS ( L" },
            offset,
        });
    };
    if mode != 48 {
        return Err(RenderError::InvalidGraphicsParameter {
            parameter: "m",
            value: u64::from(mode),
            offset,
        });
    }

    match function {
        2 | 50 if !extended_length => {
            if parameters.len() != 2 {
                return Err(RenderError::InvalidGraphicsPayloadLength {
                    expected: 2,
                    actual: parameters.len(),
                    offset,
                });
            }
            state.print_buffered_graphics(offset)
        }
        112 => store_raster_graphics(parameters, offset, state),
        function => Err(RenderError::UnsupportedGraphicsFunction { function, offset }),
    }
}

fn store_raster_graphics<S: RenderSurface>(
    parameters: &[u8],
    offset: usize,
    state: &mut PrinterState<S>,
) -> Result<(), RenderError> {
    if parameters.len() < 10 {
        return Err(RenderError::InvalidGraphicsPayloadLength {
            expected: 10,
            actual: parameters.len(),
            offset,
        });
    }
    let tone = parameters[2];
    let scale_x = parameters[3];
    let scale_y = parameters[4];
    let color = parameters[5];
    validate_graphics_parameter(tone == 48, "a", tone, offset)?;
    validate_graphics_parameter(matches!(scale_x, 1 | 2), "bx", scale_x, offset)?;
    validate_graphics_parameter(matches!(scale_y, 1 | 2), "by", scale_y, offset)?;
    validate_graphics_parameter(color == 49, "c", color, offset)?;

    let width_dots = usize::from(u16::from_le_bytes([parameters[6], parameters[7]]));
    let height_dots = usize::from(u16::from_le_bytes([parameters[8], parameters[9]]));
    if width_dots == 0 || height_dots == 0 {
        return Err(RenderError::InvalidGraphicsDimensions {
            width_dots,
            height_dots,
            offset,
        });
    }
    let row_bytes = width_dots.div_ceil(8);
    let expected_payload = row_bytes.saturating_mul(height_dots);
    let payload = &parameters[10..];
    if payload.len() != expected_payload {
        return Err(RenderError::InvalidGraphicsPayloadLength {
            expected: 10 + expected_payload,
            actual: parameters.len(),
            offset,
        });
    }

    state.store_raster_graphics(
        BufferedGraphics {
            payload: payload.to_vec(),
            row_bytes,
            width_dots,
            height_dots,
            horizontal_scale: u32::from(scale_x),
            vertical_scale: u32::from(scale_y),
        },
        offset,
    )
}

fn validate_graphics_parameter(
    valid: bool,
    parameter: &'static str,
    value: u8,
    offset: usize,
) -> Result<(), RenderError> {
    if valid {
        return Ok(());
    }
    Err(RenderError::InvalidGraphicsParameter {
        parameter,
        value: u64::from(value),
        offset,
    })
}

fn execute_gs_v<S: RenderSurface>(
    data: &[u8],
    offset: usize,
    state: &mut PrinterState<S>,
) -> Result<usize, RenderError> {
    let Some(mode) = data.get(2).copied() else {
        return Err(RenderError::TruncatedCommand {
            command: "GS V",
            offset,
        });
    };

    match mode {
        0 | 48 => {
            state.cut(false, offset)?;
            Ok(3)
        }
        1 | 49 => {
            state.cut(true, offset)?;
            Ok(3)
        }
        mode @ (65 | 66) => {
            let Some(feed) = data.get(3).copied() else {
                return Err(RenderError::TruncatedCommand {
                    command: "GS V",
                    offset,
                });
            };
            state.feed_to_cut_position_and_cut(mode, feed, offset)?;
            Ok(4)
        }
        97 | 98 | 103 | 104 => {
            if data.get(3).is_none() {
                return Err(RenderError::TruncatedCommand {
                    command: "GS V",
                    offset,
                });
            }
            Err(RenderError::UnsupportedCutMode { mode, offset })
        }
        mode => Err(RenderError::UnsupportedCutMode { mode, offset }),
    }
}

fn execute_gs_v0<S: RenderSurface>(
    data: &[u8],
    offset: usize,
    state: &mut PrinterState<S>,
) -> Result<usize, RenderError> {
    if data.len() < 3 {
        return Err(RenderError::TruncatedCommand {
            command: "GS v 0",
            offset,
        });
    }

    if data[2] != 0x30 {
        return Err(RenderError::UnsupportedGsCommand {
            command: data[1],
            offset,
        });
    }

    if !state.at_beginning_of_line {
        // In Standard mode Epson consumes only the GS v 0 prefix when the
        // line has started. The outer parser must see m and every later byte
        // as normal input instead of trusting the raster length fields.
        return Ok(3);
    }
    state.require_raster_bit_image(offset)?;

    if data.len() < 8 {
        return Err(RenderError::TruncatedCommand {
            command: "GS v 0",
            offset,
        });
    }

    let mode = data[3];
    let (horizontal_scale, vertical_scale) = match mode {
        0 | 48 => (1, 1),
        1 | 49 => (2, 1),
        2 | 50 => (1, 2),
        3 | 51 => (2, 2),
        mode => return Err(RenderError::UnsupportedRasterBitImageMode { mode, offset }),
    };

    let width_bytes = usize::from(data[4]) + usize::from(data[5]) * 256;
    let height_dots = usize::from(data[6]) + usize::from(data[7]) * 256;
    if width_bytes == 0 || height_dots == 0 {
        return Err(RenderError::InvalidRasterBitImageDimensions {
            width_bytes,
            height_dots,
            offset,
        });
    }

    let payload_length = width_bytes.saturating_mul(height_dots);
    state.validate_command_payload_size(payload_length)?;
    let command_length = 8 + payload_length;
    let Some(payload) = data.get(8..command_length) else {
        return Err(RenderError::TruncatedCommand {
            command: "GS v 0",
            offset,
        });
    };

    state.print_raster_image(
        payload,
        width_bytes,
        width_bytes.saturating_mul(8) as u32,
        height_dots,
        horizontal_scale,
        vertical_scale,
    )?;
    state.mark_gs_v_0_printed();
    Ok(command_length)
}
