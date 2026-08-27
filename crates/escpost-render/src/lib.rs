//! Dot-accurate ESC/POS rendering.

mod barcode;
mod command;
mod databar;
mod error;
mod font;
mod graphics;
mod international;
mod qr;
mod state;
mod surface;
mod symbols;
mod text;
mod trace;

pub use error::{LimitKind, RenderError, RenderWarning};
pub use surface::MonoSurface;
pub use trace::{
    CommandCode, CommandTrace, DecodedCommand, Effect, Justification, PaintLifecycle, PaintRegion,
    Position, SheetTrace, StateChange, StyleDefaults, TRACED_COMMAND_BYTES, TextFont, TextStyle,
    Trace,
};

use command::{execute_esc_command, execute_gs_command};
use escpost_profiles::PrinterProfile;
use state::PrinterState;
use surface::{RenderSurface, encode_png};
use trace::{CommandSink, NoTrace, TraceCollector};

/// Product-supported preview density in subpixels per printer dot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderScale(u32);

impl RenderScale {
    pub const MIN: u32 = 1;
    pub const MAX: u32 = 3;

    pub fn new(value: u32) -> Result<Self, InvalidRenderScale> {
        if (Self::MIN..=Self::MAX).contains(&value) {
            Ok(Self(value))
        } else {
            Err(InvalidRenderScale { value })
        }
    }

    pub fn get(self) -> u32 {
        self.0
    }
}

impl Default for RenderScale {
    fn default() -> Self {
        Self(Self::MIN)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("render scale must be between 1 and 3, got {value}")]
pub struct InvalidRenderScale {
    value: u32,
}

impl InvalidRenderScale {
    pub fn value(self) -> u32 {
        self.value
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RenderOptions {
    pub limits: RenderLimits,
    /// Subpixels per dot. `1` is dot resolution; `N > 1` renders at `N ×`
    /// density. Independent of `antialias`.
    pub scale: RenderScale,
    /// When `false`, glyph coverage is thresholded to hard dots and the sheet
    /// encodes as a faithful 1-bit PNG (the printer's real output, used by
    /// golden tests). When `true`, glyph edges keep their coverage and the sheet
    /// encodes as an 8-bit grayscale preview — cosmetic only, never what prints.
    pub antialias: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderLimits {
    pub max_input_bytes: usize,
    pub max_command_payload_bytes: usize,
    pub max_sheet_width_dots: u32,
    pub max_sheet_height_dots: u32,
    pub max_sheets: usize,
    pub max_total_dots: u64,
}

impl Default for RenderLimits {
    fn default() -> Self {
        Self {
            max_input_bytes: 16 * 1024 * 1024,
            max_command_payload_bytes: 8 * 1024 * 1024,
            max_sheet_width_dots: 4096,
            max_sheet_height_dots: 1_000_000,
            max_sheets: 32,
            max_total_dots: 200_000_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderResult {
    pub sheets: Vec<RenderedSheet>,
    pub device_events: Vec<DeviceEvent>,
    /// Non-fatal diagnostics from an otherwise successful render, such as a cut
    /// requested on a profile whose printer has no cutter.
    pub warnings: Vec<RenderWarning>,
    pub metadata: RenderMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceEvent {
    CashDrawerPulse {
        connector: u8,
        on_time_units: u8,
        off_time_units: u8,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderMetadata {
    pub renderer_version: &'static str,
    pub profile_id: String,
    pub canonical_profile_sha256: String,
    /// The style the printer profile starts a job with.
    pub style_defaults: StyleDefaults,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedSheet {
    pub surface: MonoSurface,
    pub png: Vec<u8>,
}

/// Experimental result containing both the rendered sheets and command trace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TracedRenderResult {
    pub render: RenderResult,
    pub trace: Trace,
}

pub fn render(data: &[u8], profile: &PrinterProfile) -> Result<RenderResult, RenderError> {
    render_with_options(data, profile, &RenderOptions::default())
}

pub fn render_with_options(
    data: &[u8],
    profile: &PrinterProfile,
    options: &RenderOptions,
) -> Result<RenderResult, RenderError> {
    let rendered = render_surfaces::<MonoSurface>(data, profile, options)?;
    let mut sheets = Vec::new();
    for surface in rendered.surfaces {
        let png = encode_png(&surface)?;
        sheets.push(RenderedSheet { surface, png });
    }

    Ok(RenderResult {
        sheets,
        device_events: rendered.device_events,
        warnings: rendered.warnings,
        metadata: RenderMetadata {
            renderer_version: env!("CARGO_PKG_VERSION"),
            profile_id: profile.id.clone(),
            canonical_profile_sha256: profile.canonical_profile_sha256.clone(),
            style_defaults: style_defaults(profile),
        },
    })
}

/// Render with the experimental command trace using default options.
pub fn render_with_trace(
    data: &[u8],
    profile: &PrinterProfile,
) -> Result<TracedRenderResult, RenderError> {
    render_with_trace_and_options(data, profile, &RenderOptions::default())
}

/// Render with the experimental command trace and explicit options.
pub fn render_with_trace_and_options(
    data: &[u8],
    profile: &PrinterProfile,
    options: &RenderOptions,
) -> Result<TracedRenderResult, RenderError> {
    use surface::tracing::TracingSurface;

    let mut collector = TraceCollector::default();
    let rendered =
        render_surfaces_with_sink::<TracingSurface, _>(data, profile, options, &mut collector)?;
    let trace = collector.finish(&rendered.surfaces);
    let mut sheets = Vec::new();
    for surface in rendered.surfaces {
        let png = encode_png(&surface.inner)?;
        sheets.push(RenderedSheet {
            surface: surface.inner,
            png,
        });
    }

    Ok(TracedRenderResult {
        render: RenderResult {
            sheets,
            device_events: rendered.device_events,
            warnings: rendered.warnings,
            metadata: RenderMetadata {
                renderer_version: env!("CARGO_PKG_VERSION"),
                profile_id: profile.id.clone(),
                canonical_profile_sha256: profile.canonical_profile_sha256.clone(),
                style_defaults: style_defaults(profile),
            },
        },
        trace,
    })
}

struct SurfaceRender<S> {
    surfaces: Vec<S>,
    device_events: Vec<DeviceEvent>,
    warnings: Vec<RenderWarning>,
}

fn render_surfaces<S: RenderSurface>(
    data: &[u8],
    profile: &PrinterProfile,
    options: &RenderOptions,
) -> Result<SurfaceRender<S>, RenderError> {
    render_surfaces_with_sink(data, profile, options, &mut NoTrace)
}

fn render_surfaces_with_sink<S: RenderSurface, C: CommandSink>(
    data: &[u8],
    profile: &PrinterProfile,
    options: &RenderOptions,
    command_sink: &mut C,
) -> Result<SurfaceRender<S>, RenderError> {
    validate_initial_limits(data, profile, &options.limits)?;
    let mut state = PrinterState::new(
        profile,
        options.limits,
        options.scale.get(),
        options.antialias,
    );
    let mut offset = 0;

    while offset < data.len() {
        let byte = data[offset];
        if C::ENABLED {
            state.end_command();
            state.begin_command(offset);
            command_sink.begin_command(state.trace_sheet_index(), offset);
        }
        if byte != 0x0a {
            state.clear_pending_gs_v_0_lf();
        }

        let command_length = match byte {
            0x09 => {
                state.horizontal_tab()?;
                1
            }
            0x0a => {
                trace::execute_line_feed(&mut state, command_sink)?;
                1
            }
            0x0d => {
                state.carriage_return()?;
                1
            }
            0x1b => execute_esc_command(&data[offset..], offset, &mut state, command_sink)?,
            0x1d => execute_gs_command(&data[offset..], offset, &mut state, command_sink)?,
            // ESC/POS code pages retain ASCII in 20h–7Eh and assign printable
            // characters to 80h–FFh. Control bytes remain parser input.
            byte @ (0x20..=0x7e | 0x80..=0xff) => {
                trace::execute_text_byte(&mut state, command_sink, byte, offset)?;
                1
            }
            byte => return Err(RenderError::UnsupportedDataByte { byte, offset }),
        };
        if C::ENABLED {
            let end = (offset + command_length).min(data.len());
            command_sink.finish_command(
                &data[offset..end],
                state.trace_paint_lifecycle(offset),
                state.trace_text_style(),
            );
        }
        offset += command_length;
    }

    let device_events = std::mem::take(&mut state.device_events);
    let warnings = std::mem::take(&mut state.warnings);
    Ok(SurfaceRender {
        surfaces: state.into_surfaces()?,
        device_events,
        warnings,
    })
}

/// The style a printer profile starts a job with.
fn style_defaults(profile: &PrinterProfile) -> StyleDefaults {
    StyleDefaults {
        line_spacing_dots: profile.defaults.line_spacing_dots,
        code_page: profile.defaults.code_page,
        international_character_set: profile.defaults.international_character_set,
    }
}

fn validate_initial_limits(
    data: &[u8],
    profile: &PrinterProfile,
    limits: &RenderLimits,
) -> Result<(), RenderError> {
    if data.len() > limits.max_input_bytes {
        return Err(RenderError::LimitExceeded {
            kind: LimitKind::InputBytes,
            value: data.len() as u64,
            limit: limits.max_input_bytes as u64,
        });
    }
    if profile.geometry.printable_width_dots > limits.max_sheet_width_dots {
        return Err(RenderError::LimitExceeded {
            kind: LimitKind::SheetWidthDots,
            value: u64::from(profile.geometry.printable_width_dots),
            limit: u64::from(limits.max_sheet_width_dots),
        });
    }
    Ok(())
}
