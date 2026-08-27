//! Character decoding and glyph placement.

use crate::font;
use crate::state::PrinterState;
use crate::surface::RenderSurface;
use crate::{RenderError, international};
use encoding_rs::{
    Encoding, WINDOWS_1250, WINDOWS_1251, WINDOWS_1252, WINDOWS_1253, WINDOWS_1254, WINDOWS_1255,
    WINDOWS_1256, WINDOWS_1257, WINDOWS_1258,
};
use escpost_profiles::Font as ProfileFont;
use oem_cp::{
    Cp437, Cp720, Cp737, Cp775, Cp850, Cp852, Cp855, Cp857, Cp858, Cp860, Cp861, Cp862, Cp863,
    Cp864, Cp865, Cp866, Cp869, Cp874,
};

impl<S: RenderSurface> PrinterState<S> {
    pub(crate) fn print_byte(&mut self, byte: u8, offset: usize) -> Result<(), RenderError> {
        // The ESC t operand is printer-specific. The profile translates that
        // numeric slot into a stable encoding name before we decode the byte.
        let encoding = self
            .code_page_encoding(self.active_code_page)
            .unwrap_or("<not present in printer profile>");

        // ESC R replaces a small set of ASCII positions independently of the
        // active ESC t table. Printable ASCII is common to every table,
        // including multibyte tables whose extended ranges remain post-v1.
        let character = international::substitution(self.active_international_character_set, byte)
            .or_else(|| byte.is_ascii().then(|| char::from(byte)))
            .or_else(|| {
                is_supported_code_page_encoding(encoding)
                    .then(|| decode_printable_byte(byte, encoding))
                    .flatten()
            });
        if character.is_none() && !is_supported_code_page_encoding(encoding) {
            return Err(RenderError::UnsupportedCodePage {
                code_page: self.active_code_page,
                encoding: encoding.to_owned(),
                offset,
            });
        }
        let Some(character) = character else {
            return Err(RenderError::UndefinedCodePageByte {
                byte,
                code_page: self.active_code_page,
                encoding: encoding.to_owned(),
                offset,
            });
        };
        // fontdue uses glyph index zero for the font's generic .notdef box.
        // Report it instead of silently making unrelated scripts look equal.
        if font::default_font().lookup_glyph_index(character) == 0 {
            return Err(RenderError::MissingGlyph { character, offset });
        }

        self.print_character(character)
    }

    pub(crate) fn print_character(&mut self, character: char) -> Result<(), RenderError> {
        let cell_width = self.current_character_advance_width();
        // The cell of the active font is read once here, because drawing the
        // glyph needs the printer itself and cannot hold the font at the same
        // time.
        let font = self.font();
        let (font_width, font_height, baseline) = (
            font.cell_width_dots,
            font.cell_height_dots,
            font.baseline_dots,
        );
        let cell_height = font_height.saturating_mul(self.character_height_multiplier);
        if self.print_x.saturating_add(cell_width) > self.line.width() {
            self.line_feed()?;
        }
        self.line_height = self.line_height.max(cell_height);
        self.line
            .mark_region(self.print_x, 0, cell_width, cell_height);
        if self.reversed {
            for x in self.print_x..self.print_x.saturating_add(cell_width) {
                for y in 0..cell_height {
                    self.line.print_dot(x, y);
                }
            }
        }

        self.draw_glyph(character, font_width, font_height, baseline);

        if !self.reversed {
            let underline_top = cell_height.saturating_sub(self.underline_thickness);
            for x in self.print_x..self.print_x.saturating_add(cell_width) {
                for y in underline_top..cell_height {
                    self.line.print_dot(x, y);
                }
            }
        }

        self.print_x = self.print_x.saturating_add(cell_width);
        self.line_used_width = self.line_used_width.max(self.print_x);
        self.line_has_printable_data = true;
        self.at_beginning_of_line = false;
        Ok(())
    }

    /// Blit one glyph into the line surface.
    ///
    /// Magnified glyphs (`GS !` / `ESC !`) are rasterized at their true magnified
    /// size rather than block-doubling the base cell, so large text stays crisp:
    /// the scalable substitute font renders each size at native quality, and the
    /// condense mechanism handles anisotropic sizes (double-width stretches,
    /// double-height narrows). Without anti-aliasing each subpixel is thresholded
    /// to a hard dot (the faithful path — what prints); with it the soft coverage
    /// is kept. Ink is laid over paper or carved out of a reverse block, and
    /// emphasis smears one dot right.
    fn draw_glyph(
        &mut self,
        character: char,
        cell_width_dots: u32,
        cell_height_dots: u32,
        baseline_dots: u32,
    ) {
        let scale = self.scale;
        // Rasterize into the magnified cell so the glyph is drawn at its real
        // size, not an upscale of the base cell.
        let glyph_width_dots = cell_width_dots.saturating_mul(self.character_width_multiplier);
        let glyph_height_dots = cell_height_dots.saturating_mul(self.character_height_multiplier);
        let geometry = font::glyph_geometry(
            glyph_width_dots,
            glyph_height_dots,
            baseline_dots.saturating_mul(self.character_height_multiplier),
        );
        let coverage = font::glyph_cell_coverage(
            character,
            glyph_width_dots,
            glyph_height_dots,
            &geometry,
            scale,
        );
        let hard = |sample: u8| sample >= font::GLYPH_ALPHA_THRESHOLD;
        let inked = |sample: u8| {
            if self.antialias {
                sample != 0
            } else {
                hard(sample)
            }
        };
        if !coverage.iter().copied().any(inked) {
            return;
        }
        let source_width = (glyph_width_dots * scale) as usize;
        let source_height = (glyph_height_dots * scale) as usize;
        // Reserve the cell rows so subpixel blends land in-bounds.
        self.line.ensure_height(glyph_height_dots);
        // Lay ink over paper for normal text; carve paper from a reverse block.
        let lay_ink = !self.reversed;
        let base_x = self.print_x * scale;
        let cell_right = self
            .print_x
            .saturating_add(self.current_character_advance_width())
            * scale;
        for source_y in 0..source_height {
            for source_x in 0..source_width {
                let sample = coverage[source_y * source_width + source_x];
                // Faithful path snaps to full ink at the glyph threshold; the
                // preview keeps the soft coverage.
                let value = if self.antialias {
                    sample
                } else if hard(sample) {
                    255
                } else {
                    0
                };
                if value == 0 {
                    continue;
                }
                let dx = base_x + source_x as u32;
                let dy = source_y as u32;
                self.line.blend_subpixel(dx, dy, value, lay_ink);
                // Emphasis double-strike: one dot to the right, staying in-cell.
                if self.emphasized && dx + scale < cell_right {
                    self.line.blend_subpixel(dx + scale, dy, value, lay_ink);
                }
            }
        }
    }

    pub(crate) fn current_character_advance_width(&self) -> u32 {
        self.font()
            .cell_width_dots
            .saturating_add(self.right_side_character_spacing)
            .saturating_mul(self.character_width_multiplier)
    }
}

pub(crate) fn render_hri<S: RenderSurface>(
    data: &[char],
    profile_font: &ProfileFont,
    template: &S,
    scale: u32,
    antialias: bool,
) -> S {
    let width = (data.len() as u32).saturating_mul(profile_font.cell_width_dots);
    let mut surface = template.fork(width);
    surface.ensure_height(profile_font.cell_height_dots);

    // Map glyphs onto the cell exactly like printed text so HRI labels share the
    // same shapes; the geometry is constant across the label.
    let geometry = font::glyph_geometry(
        profile_font.cell_width_dots,
        profile_font.cell_height_dots,
        profile_font.baseline_dots,
    );
    let cell_width_dots = profile_font.cell_width_dots;
    let cell_height_dots = profile_font.cell_height_dots;
    let source_width = (cell_width_dots * scale) as usize;
    let source_height = (cell_height_dots * scale) as usize;

    for (character_index, character) in data.iter().copied().enumerate() {
        let base_x = character_index as u32 * cell_width_dots * scale;
        let coverage = font::glyph_cell_coverage(
            character,
            cell_width_dots,
            cell_height_dots,
            &geometry,
            scale,
        );
        for source_y in 0..source_height {
            for source_x in 0..source_width {
                let sample = coverage[source_y * source_width + source_x];
                // Faithful HRI snaps to full ink at the glyph threshold; the
                // preview keeps the soft coverage.
                let value = if antialias {
                    sample
                } else if sample >= font::GLYPH_ALPHA_THRESHOLD {
                    255
                } else {
                    0
                };
                if value != 0 {
                    surface.blend_subpixel(base_x + source_x as u32, source_y as u32, value, true);
                }
            }
        }
    }
    surface
}

fn is_supported_code_page_encoding(encoding: &str) -> bool {
    matches!(
        encoding,
        "CP437"
            | "CP720"
            | "CP737"
            | "CP775"
            | "CP850"
            | "CP852"
            | "CP855"
            | "CP857"
            | "CP858"
            | "CP860"
            | "CP861"
            | "CP862"
            | "CP863"
            | "CP864"
            | "CP865"
            | "CP866"
            | "CP869"
            | "CP874"
            | "CP1250"
            | "CP1251"
            | "CP1252"
            | "CP1253"
            | "CP1254"
            | "CP1255"
            | "CP1256"
            | "CP1257"
            | "CP1258"
    )
}

fn decode_printable_byte(byte: u8, encoding: &str) -> Option<char> {
    match encoding {
        "CP437" => Some(char::from(Cp437::from(byte))),
        "CP720" => Some(char::from(Cp720::from(byte))),
        "CP737" => Some(char::from(Cp737::from(byte))),
        "CP775" => Some(char::from(Cp775::from(byte))),
        "CP850" => Some(char::from(Cp850::from(byte))),
        "CP852" => Some(char::from(Cp852::from(byte))),
        "CP855" => Some(char::from(Cp855::from(byte))),
        "CP857" => Cp857::try_from(byte).ok().map(char::from),
        "CP858" => Some(char::from(Cp858::from(byte))),
        "CP860" => Some(char::from(Cp860::from(byte))),
        "CP861" => Some(char::from(Cp861::from(byte))),
        "CP862" => Some(char::from(Cp862::from(byte))),
        "CP863" => Some(char::from(Cp863::from(byte))),
        "CP864" => Cp864::try_from(byte).ok().map(char::from),
        "CP865" => Some(char::from(Cp865::from(byte))),
        "CP866" => Some(char::from(Cp866::from(byte))),
        "CP869" => Some(char::from(Cp869::from(byte))),
        "CP874" => Cp874::try_from(byte).ok().map(char::from),
        "CP1250" => decode_with_encoding_rs(byte, WINDOWS_1250),
        "CP1251" => decode_with_encoding_rs(byte, WINDOWS_1251),
        "CP1252" => decode_with_encoding_rs(byte, WINDOWS_1252),
        "CP1253" => decode_with_encoding_rs(byte, WINDOWS_1253),
        "CP1254" => decode_with_encoding_rs(byte, WINDOWS_1254),
        "CP1255" => decode_with_encoding_rs(byte, WINDOWS_1255),
        "CP1256" => decode_with_encoding_rs(byte, WINDOWS_1256),
        "CP1257" => decode_with_encoding_rs(byte, WINDOWS_1257),
        "CP1258" => decode_with_encoding_rs(byte, WINDOWS_1258),
        _ => unreachable!("code-page support is checked when ESC t is executed"),
    }
}

fn decode_with_encoding_rs(byte: u8, encoding: &'static Encoding) -> Option<char> {
    let bytes = [byte];
    let (decoded, had_errors) = encoding.decode_without_bom_handling(&bytes);

    (!had_errors).then(|| decoded.chars().next()).flatten()
}
