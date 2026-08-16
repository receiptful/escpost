//! Printer state: motion, margins, fonts, line buffering, and sheets.

use crate::surface::{MonoSurface, RenderSurface};
use crate::symbols::barcode_system_command_name;
use crate::trace::PaintLifecycle;
use crate::{DeviceEvent, LimitKind, RenderError, RenderLimits, RenderWarning, qr};
use escpost_profiles::{
    BarcodeSystem, CarriageReturnMode, FeedBehavior, Font as ProfileFont, PositioningBehavior,
    PrinterProfile,
};
use std::collections::{BTreeMap, BTreeSet};

const DEFAULT_BARCODE_HEIGHT_DOTS: u32 = 162;
const DEFAULT_BARCODE_MODULE_WIDTH_DOTS: u32 = 3;
const DEFAULT_QR_MODULE_SIZE_DOTS: u32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Justification {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum HriPosition {
    #[default]
    None,
    Above,
    Below,
    AboveAndBelow,
}

#[derive(Debug, Clone)]
pub(crate) struct BufferedGraphics {
    pub(crate) payload: Vec<u8>,
    pub(crate) row_bytes: usize,
    pub(crate) width_dots: usize,
    pub(crate) height_dots: usize,
    pub(crate) horizontal_scale: u32,
    pub(crate) vertical_scale: u32,
}

#[derive(Debug)]
pub(crate) struct PrinterState<S: RenderSurface = MonoSurface> {
    pub(crate) profile_id: String,
    pub(crate) limits: RenderLimits,
    pub(crate) device_events: Vec<DeviceEvent>,
    pub(crate) warnings: Vec<RenderWarning>,
    pub(crate) completed_sheets: Vec<S>,
    // Subpixels per dot and whether glyph edges stay soft (grayscale preview).
    pub(crate) scale: u32,
    pub(crate) antialias: bool,
    pub(crate) roll: S,
    // Text and ESC * data are composed on a line first because ESC a applies
    // justification when the printer receives the line feed, not per glyph.
    pub(crate) line: S,
    pub(crate) print_area_left: u32,
    pub(crate) print_area_width: u32,
    pub(crate) line_top: u32,
    pub(crate) print_x: u32,
    pub(crate) line_used_width: u32,
    // Cursor movement can increase line_used_width without buffering data.
    // Keep this separate for firmware rules that distinguish those states.
    pub(crate) line_has_printable_data: bool,
    // Some commands are deliberately ignored after printable data or a
    // position command has moved the printer away from the line origin.
    pub(crate) at_beginning_of_line: bool,
    pub(crate) line_spacing: u32,
    pub(crate) default_line_spacing: u32,
    pub(crate) horizontal_dpi: u32,
    pub(crate) default_horizontal_motion_units_per_inch: u32,
    pub(crate) horizontal_motion_units_per_inch: u32,
    pub(crate) vertical_dpi: u32,
    pub(crate) default_vertical_motion_units_per_inch: u32,
    pub(crate) vertical_motion_units_per_inch: u32,
    pub(crate) esc_star_8_dot_vertical_pitch: u32,
    pub(crate) esc_backslash_negative_behavior: PositioningBehavior,
    pub(crate) esc_dollar_after_printable_data_behavior: PositioningBehavior,
    pub(crate) esc_j_behavior: FeedBehavior,
    pub(crate) gs_v_0_following_lf_behavior: FeedBehavior,
    pub(crate) pending_gs_v_0_lf: bool,
    pub(crate) gs_v_function_b_full_behavior: FeedBehavior,
    pub(crate) gs_v_function_b_partial_behavior: FeedBehavior,
    pub(crate) print_head_to_cutter_dots: Option<u32>,
    pub(crate) font_a: ProfileFont,
    pub(crate) font_b: ProfileFont,
    pub(crate) active_font: ProfileFont,
    pub(crate) code_pages: BTreeMap<u8, String>,
    pub(crate) default_code_page: u8,
    pub(crate) active_code_page: u8,
    pub(crate) default_international_character_set: u8,
    pub(crate) active_international_character_set: u8,
    pub(crate) carriage_return_mode: CarriageReturnMode,
    pub(crate) right_side_character_spacing: u32,
    pub(crate) default_tab_positions: Vec<u32>,
    pub(crate) tab_positions: Vec<u32>,
    pub(crate) character_width_multiplier: u32,
    pub(crate) character_height_multiplier: u32,
    pub(crate) emphasized: bool,
    pub(crate) underline_thickness: u32,
    pub(crate) reversed: bool,
    pub(crate) justification: Justification,
    pub(crate) line_height: u32,
    pub(crate) buffered_graphics: Option<BufferedGraphics>,
    pub(crate) stored_qr_data: Option<Vec<u8>>,
    pub(crate) qr_module_size: u32,
    pub(crate) qr_error_correction: qr::ErrorCorrection,
    pub(crate) barcode_height: u32,
    pub(crate) barcode_module_width: u32,
    pub(crate) hri_position: HriPosition,
    pub(crate) hri_font: ProfileFont,
    pub(crate) function_a_barcodes: BTreeSet<BarcodeSystem>,
    pub(crate) function_b_barcodes: BTreeSet<BarcodeSystem>,
    pub(crate) supports_qr: bool,
    pub(crate) supports_column_bit_image: bool,
    pub(crate) supports_raster_bit_image: bool,
    pub(crate) supports_graphics: bool,
    pub(crate) supports_full_cut: bool,
    pub(crate) supports_partial_cut: bool,
    pub(crate) supports_standard_drawer_pulse: bool,
}

impl<S: RenderSurface> PrinterState<S> {
    pub(crate) fn trace_paint_lifecycle(&self, command_offset: usize) -> Option<PaintLifecycle> {
        if self.roll.has_command_region(command_offset) {
            Some(PaintLifecycle::Committed)
        } else if self.line.has_command_region(command_offset) {
            Some(PaintLifecycle::Buffered)
        } else {
            None
        }
    }

    pub(crate) fn trace_justification(&self) -> Justification {
        self.justification
    }

    pub(crate) fn trace_qr_data(&self) -> &[u8] {
        self.stored_qr_data.as_deref().unwrap_or_default()
    }

    pub(crate) fn trace_position(&self) -> (u32, u32) {
        (
            self.print_area_left.saturating_add(self.print_x),
            self.line_top,
        )
    }

    pub(crate) fn trace_line_feed_start_position(&self) -> (u32, u32) {
        let line_left = if self.line_used_width == 0 {
            0
        } else {
            let remaining_width = self.line.width().saturating_sub(self.line_used_width);
            match self.justification {
                Justification::Left => 0,
                Justification::Center => remaining_width / 2,
                Justification::Right => remaining_width,
            }
        };
        (
            self.print_area_left
                .saturating_add(line_left)
                .saturating_add(self.print_x),
            self.line_top,
        )
    }

    pub(crate) fn trace_sheet_index(&self) -> usize {
        self.completed_sheets.len()
    }

    pub(crate) fn new(
        profile: &PrinterProfile,
        limits: RenderLimits,
        scale: u32,
        antialias: bool,
    ) -> Self {
        let width = profile.geometry.printable_width_dots;
        let default_line_spacing = profile.defaults.line_spacing_dots;
        let font_a = profile.fonts.a.clone();
        let font_b = profile.fonts.b.clone();
        let hri_font = font_a.clone();
        // ESC/POS defaults to columns 8, 16, ... 248 measured with the
        // power-on font and size.
        let default_tab_positions = (1..=31)
            .map(|index| index * 8 * font_a.cell_width_dots)
            .collect::<Vec<_>>();

        Self {
            profile_id: profile.id.clone(),
            limits,
            device_events: Vec::new(),
            warnings: Vec::new(),
            completed_sheets: Vec::new(),
            scale,
            antialias,
            roll: S::new(width, scale, antialias),
            line: S::new(width, scale, antialias),
            print_area_left: 0,
            print_area_width: width,
            line_top: 0,
            print_x: 0,
            line_used_width: 0,
            line_has_printable_data: false,
            at_beginning_of_line: true,
            line_spacing: default_line_spacing,
            default_line_spacing,
            horizontal_dpi: profile.geometry.dpi_x,
            default_horizontal_motion_units_per_inch: profile.motion.horizontal_units_per_inch,
            horizontal_motion_units_per_inch: profile.motion.horizontal_units_per_inch,
            vertical_dpi: profile.geometry.dpi_y,
            default_vertical_motion_units_per_inch: profile.motion.vertical_units_per_inch,
            vertical_motion_units_per_inch: profile.motion.vertical_units_per_inch,
            esc_star_8_dot_vertical_pitch: profile.column_bit_image.eight_dot_vertical_pitch_dots,
            esc_backslash_negative_behavior: profile.commands.esc_backslash_negative,
            esc_dollar_after_printable_data_behavior: profile
                .commands
                .esc_dollar_after_printable_data,
            esc_j_behavior: profile.commands.esc_j,
            gs_v_0_following_lf_behavior: profile.commands.gs_v_0_following_lf,
            pending_gs_v_0_lf: false,
            gs_v_function_b_full_behavior: profile.commands.gs_v_function_b_full,
            gs_v_function_b_partial_behavior: profile.commands.gs_v_function_b_partial,
            print_head_to_cutter_dots: profile
                .cutter
                .as_ref()
                .map(|cutter| cutter.print_head_to_cutter_dots),
            active_font: font_a.clone(),
            font_a,
            font_b,
            code_pages: profile.code_pages.clone(),
            default_code_page: profile.defaults.code_page,
            active_code_page: profile.defaults.code_page,
            default_international_character_set: profile.defaults.international_character_set,
            active_international_character_set: profile.defaults.international_character_set,
            carriage_return_mode: profile.defaults.carriage_return,
            right_side_character_spacing: 0,
            tab_positions: default_tab_positions.clone(),
            default_tab_positions,
            character_width_multiplier: 1,
            character_height_multiplier: 1,
            emphasized: false,
            underline_thickness: 0,
            reversed: false,
            justification: Justification::Left,
            line_height: 0,
            buffered_graphics: None,
            stored_qr_data: None,
            qr_module_size: DEFAULT_QR_MODULE_SIZE_DOTS,
            qr_error_correction: qr::ErrorCorrection::Low,
            barcode_height: DEFAULT_BARCODE_HEIGHT_DOTS,
            barcode_module_width: DEFAULT_BARCODE_MODULE_WIDTH_DOTS,
            hri_position: HriPosition::None,
            hri_font,
            function_a_barcodes: profile.features.barcodes.function_a.clone(),
            function_b_barcodes: profile.features.barcodes.function_b.clone(),
            supports_qr: profile.features.qr_code,
            supports_column_bit_image: profile.features.bit_image_column,
            supports_raster_bit_image: profile.features.bit_image_raster,
            supports_graphics: profile.features.graphics,
            supports_full_cut: profile.features.paper_full_cut,
            supports_partial_cut: profile.features.paper_part_cut,
            supports_standard_drawer_pulse: profile.features.pulse_standard,
        }
    }

    pub(crate) fn begin_command(&mut self, offset: usize) {
        self.roll.begin_command(offset);
        self.line.begin_command(offset);
    }

    pub(crate) fn end_command(&mut self) {
        self.roll.end_command();
        self.line.end_command();
    }

    pub(crate) fn initialize(&mut self) {
        // Epson defines ESC @ as clearing the print buffer before restoring
        // modes. Already committed rows on `roll` represent fed paper and stay.
        self.print_area_left = 0;
        self.print_area_width = self.roll.width();
        self.line = self.roll.fork(self.print_area_width);
        self.print_x = 0;
        self.line_used_width = 0;
        self.line_has_printable_data = false;
        self.at_beginning_of_line = true;
        self.line_spacing = self.default_line_spacing;
        self.horizontal_motion_units_per_inch = self.default_horizontal_motion_units_per_inch;
        self.vertical_motion_units_per_inch = self.default_vertical_motion_units_per_inch;
        self.active_font = self.font_a.clone();
        self.active_code_page = self.default_code_page;
        self.active_international_character_set = self.default_international_character_set;
        self.right_side_character_spacing = 0;
        self.tab_positions.clone_from(&self.default_tab_positions);
        self.character_width_multiplier = 1;
        self.character_height_multiplier = 1;
        self.emphasized = false;
        self.underline_thickness = 0;
        self.reversed = false;
        self.justification = Justification::Left;
        self.line_height = 0;
        self.buffered_graphics = None;
        self.pending_gs_v_0_lf = false;
        self.stored_qr_data = None;
        self.qr_module_size = DEFAULT_QR_MODULE_SIZE_DOTS;
        self.qr_error_correction = qr::ErrorCorrection::Low;
        self.barcode_height = DEFAULT_BARCODE_HEIGHT_DOTS;
        self.barcode_module_width = DEFAULT_BARCODE_MODULE_WIDTH_DOTS;
        self.hri_position = HriPosition::None;
        self.hri_font = self.font_a.clone();
    }

    pub(crate) fn set_print_mode(&mut self, mode: u8) {
        if mode & 0x01 == 0 {
            self.select_font_a();
        } else {
            self.select_font_b();
        }
        self.character_height_multiplier = if mode & 0x10 == 0 { 1 } else { 2 };
        self.character_width_multiplier = if mode & 0x20 == 0 { 1 } else { 2 };
        self.emphasized = mode & 0x08 != 0;
        self.underline_thickness = u32::from(mode & 0x80 != 0);
    }

    pub(crate) fn set_character_size(&mut self, size: u8) {
        // GS ! stores height minus one in bits 0–2 and width minus one in
        // bits 4–6. Bits 3 and 7 are reserved and do not affect either value.
        self.character_height_multiplier = u32::from(size & 0x07) + 1;
        self.character_width_multiplier = u32::from((size >> 4) & 0x07) + 1;
    }

    pub(crate) fn set_barcode_height(&mut self, height: u8) {
        self.barcode_height = u32::from(height);
    }

    pub(crate) fn set_barcode_module_width(&mut self, width: u8) {
        self.barcode_module_width = u32::from(width);
    }

    pub(crate) fn set_hri_position(&mut self, position: HriPosition) {
        self.hri_position = position;
    }

    pub(crate) fn select_hri_font_a(&mut self) {
        self.hri_font = self.font_a.clone();
    }

    pub(crate) fn select_hri_font_b(&mut self) {
        self.hri_font = self.font_b.clone();
    }

    pub(crate) fn set_absolute_print_position(&mut self, motion_units: u16) {
        if self.line_has_printable_data
            && self.esc_dollar_after_printable_data_behavior == PositioningBehavior::Ignored
        {
            self.at_beginning_of_line = false;
            return;
        }

        let position = self.horizontal_motion_units_to_dots(motion_units);
        // Epson specifies that out-of-area settings are ignored, leaving the
        // previous cursor untouched.
        if position <= self.line.width() {
            self.print_x = position;
        }
        self.at_beginning_of_line = false;
    }

    pub(crate) fn set_right_side_character_spacing(&mut self, motion_units: u8) {
        self.right_side_character_spacing =
            self.horizontal_motion_units_to_dots(u16::from(motion_units));
    }

    pub(crate) fn set_motion_units(&mut self, horizontal: u8, vertical: u8) {
        self.horizontal_motion_units_per_inch = match horizontal {
            0 => self.default_horizontal_motion_units_per_inch,
            horizontal => u32::from(horizontal),
        };
        self.vertical_motion_units_per_inch = match vertical {
            0 => self.default_vertical_motion_units_per_inch,
            vertical => u32::from(vertical),
        };
    }

    pub(crate) fn set_left_margin(&mut self, motion_units: u16) {
        if !self.at_beginning_of_line {
            return;
        }

        let margin = self
            .horizontal_motion_units_to_dots(motion_units)
            .min(self.roll.width());
        self.print_area_left = margin;
        self.print_area_width = self
            .print_area_width
            .min(self.roll.width().saturating_sub(margin));
        // Line coordinates are relative to the active print area. Rebuilding
        // is safe here because GS L is honored only at the beginning of a line.
        self.line = self.roll.fork(self.print_area_width);
        self.print_x = 0;
        self.line_used_width = 0;
        self.line_has_printable_data = false;
    }

    pub(crate) fn set_print_area_width(&mut self, motion_units: u16) {
        if !self.at_beginning_of_line {
            return;
        }

        let available_width = self.roll.width().saturating_sub(self.print_area_left);
        self.print_area_width = self
            .horizontal_motion_units_to_dots(motion_units)
            .min(available_width);
        // Keeping the line buffer print-area-sized makes wrapping and
        // justification independent of the physical left margin.
        self.line = self.roll.fork(self.print_area_width);
        self.print_x = 0;
        self.line_used_width = 0;
        self.line_has_printable_data = false;
    }

    pub(crate) fn horizontal_tab(&mut self) -> Result<(), RenderError> {
        let next_position = self
            .tab_positions
            .iter()
            .copied()
            .find(|&position| position > self.print_x);

        match next_position {
            Some(position) if position <= self.line.width() => {
                self.print_x = position;
                self.line_used_width = self.line_used_width.max(position);
            }
            Some(_) => {
                // Epson performs buffer-full printing and applies HT again
                // from the next line when the next stop is outside the area.
                self.line_feed()?;
                if let Some(position) = self
                    .tab_positions
                    .iter()
                    .copied()
                    .find(|&position| position <= self.line.width())
                {
                    self.print_x = position;
                    self.line_used_width = position;
                }
            }
            None => {}
        }

        self.at_beginning_of_line = false;
        Ok(())
    }

    pub(crate) fn set_tab_positions(&mut self, columns: &[u8]) {
        let character_advance = self.current_character_advance_width();
        self.tab_positions = columns
            .iter()
            .map(|&column| u32::from(column).saturating_mul(character_advance))
            .collect();
    }

    pub(crate) fn set_relative_print_position(&mut self, motion_units: i16) {
        if motion_units.is_negative()
            && self.esc_backslash_negative_behavior == PositioningBehavior::Ignored
        {
            self.at_beginning_of_line = false;
            return;
        }

        let distance = self.horizontal_motion_units_to_dots(motion_units.unsigned_abs());
        let position = if motion_units.is_negative() {
            self.print_x.checked_sub(distance)
        } else {
            self.print_x.checked_add(distance)
        };

        // Moving left of the print-area origin or right of its edge is an
        // ignored setting, not a clamped position.
        if let Some(position) = position.filter(|&position| position <= self.line.width()) {
            self.print_x = position;
        }
        self.at_beginning_of_line = false;
    }

    pub(crate) fn horizontal_motion_units_to_dots(&self, motion_units: u16) -> u32 {
        // ESC/POS applies the current motion unit when it receives the
        // command. Store the resulting dot coordinate so later GS P changes
        // cannot move content that was already positioned.
        (u64::from(motion_units) * u64::from(self.horizontal_dpi)
            / u64::from(self.horizontal_motion_units_per_inch)) as u32
    }

    pub(crate) fn set_line_spacing(&mut self, motion_units: u8) {
        // ESC 3 uses the printer's vertical motion unit, which is not
        // necessarily one dot. Integer truncation matches dot-grid hardware.
        self.line_spacing = (u64::from(motion_units) * u64::from(self.vertical_dpi)
            / u64::from(self.vertical_motion_units_per_inch)) as u32;
    }

    pub(crate) fn restore_default_line_spacing(&mut self) {
        self.line_spacing = self.default_line_spacing;
    }

    pub(crate) fn print_and_feed_motion_units(
        &mut self,
        motion_units: u8,
    ) -> Result<(), RenderError> {
        let feed_dots = (u64::from(motion_units) * u64::from(self.vertical_dpi)
            / u64::from(self.vertical_motion_units_per_inch)) as u32;
        self.print_and_feed_dots(feed_dots)
    }

    pub(crate) fn print_and_feed_dots(&mut self, feed_dots: u32) -> Result<(), RenderError> {
        // One-off feeds reuse the normal line commit so tall data cannot
        // overlap, then restore the persistent ESC 2/ESC 3 spacing.
        let line_spacing = self.line_spacing;
        self.line_spacing = feed_dots;
        self.feed_lines(1)?;
        self.line_spacing = line_spacing;
        Ok(())
    }

    pub(crate) fn execute_esc_j(&mut self, motion_units: u8) -> Result<(), RenderError> {
        match self.esc_j_behavior {
            FeedBehavior::Feed => self.print_and_feed_motion_units(motion_units),
            FeedBehavior::Ignored => Ok(()),
        }
    }

    /// Emphasized text is rendered as a one-dot horizontal double-strike of the
    /// base glyph, matching how printer firmware implements `ESC E`. See the
    /// smear in `text.rs::print_character`.
    pub(crate) fn set_emphasis(&mut self, emphasized: bool) {
        self.emphasized = emphasized;
    }

    pub(crate) fn set_underline(&mut self, thickness: u32) {
        self.underline_thickness = thickness;
    }

    pub(crate) fn set_reverse(&mut self, reversed: bool) {
        self.reversed = reversed;
    }

    pub(crate) fn set_justification(&mut self, justification: Justification) {
        if self.at_beginning_of_line {
            self.justification = justification;
        }
    }

    pub(crate) fn select_font_a(&mut self) {
        self.active_font = self.font_a.clone();
    }

    pub(crate) fn select_font_b(&mut self) {
        self.active_font = self.font_b.clone();
    }

    pub(crate) fn code_page_encoding(&self, code_page: u8) -> Option<&str> {
        self.code_pages.get(&code_page).map(String::as_str)
    }

    pub(crate) fn select_code_page(&mut self, code_page: u8) {
        self.active_code_page = code_page;
    }

    pub(crate) fn select_international_character_set(&mut self, character_set: u8) {
        self.active_international_character_set = character_set;
    }

    pub(crate) fn feed_lines(&mut self, lines: u8) -> Result<(), RenderError> {
        let remaining_width = self.line.width().saturating_sub(self.line_used_width);
        // Track logical data width rather than scanning black dots. This keeps
        // spaces significant and preserves far-right data after ESC $ or
        // ESC \ moves the cursor back to an earlier position.
        let line_left = match self.justification {
            Justification::Left => 0,
            Justification::Center => remaining_width / 2,
            Justification::Right => remaining_width,
        };
        let feed = match lines {
            0 => 0,
            lines => self.line_spacing.max(self.line_height).saturating_add(
                self.line_spacing
                    .saturating_mul(u32::from(lines).saturating_sub(1)),
            ),
        };
        let required_height = self
            .line_top
            .saturating_add(feed.max(self.line.height()).max(self.line_height));
        self.validate_roll_height(required_height)?;

        self.roll.composite_at(
            &self.line,
            self.print_area_left.saturating_add(line_left),
            self.line_top,
        );
        // Epson expands the feed for tall characters, but ESC * graphics keep
        // the selected line spacing. This permits the intentional overlap used
        // by column-image streams whose rows are advanced separately.
        self.line_top = self.line_top.saturating_add(feed);
        self.roll.ensure_height(self.line_top);
        self.line.clear();
        self.print_x = 0;
        self.line_used_width = 0;
        self.line_has_printable_data = false;
        self.at_beginning_of_line = true;
        self.line_height = 0;
        Ok(())
    }

    pub(crate) fn feed_to_cut_position_and_cut(
        &mut self,
        mode: u8,
        feed: u8,
        offset: usize,
    ) -> Result<(), RenderError> {
        if !self.at_beginning_of_line {
            return Ok(());
        }

        let behavior = if mode == 65 {
            self.gs_v_function_b_full_behavior
        } else {
            self.gs_v_function_b_partial_behavior
        };
        if behavior == FeedBehavior::Ignored {
            return Ok(());
        }

        if !self.supports_full_cut && !self.supports_partial_cut {
            // The mechanism has no cutter: it performs only the explicit n-unit
            // feed and cannot cut. cut() still splits the preview at the boundary
            // and records that no physical cut happened.
            self.print_and_feed_motion_units(feed)?;
            return self.cut(mode == 66, offset);
        }

        let Some(print_head_to_cutter_dots) = self.print_head_to_cutter_dots else {
            // Compiled profiles reject this combination. Keep rendering
            // defensive because PrinterProfile is also a public Rust value.
            return Err(RenderError::UnsupportedCutMode { mode, offset });
        };
        let explicit_feed_dots = (u64::from(feed) * u64::from(self.vertical_dpi)
            / u64::from(self.vertical_motion_units_per_inch))
            as u32;
        self.print_and_feed_dots(print_head_to_cutter_dots.saturating_add(explicit_feed_dots))?;
        self.cut(mode == 66, offset)
    }

    pub(crate) fn cut(&mut self, partial: bool, offset: usize) -> Result<(), RenderError> {
        if !self.at_beginning_of_line {
            return Ok(());
        }

        let supported = if partial {
            self.supports_partial_cut
        } else {
            self.supports_full_cut
        };
        if !supported {
            // The printer has no matching cutter, so the paper is not physically
            // cut. Still split the preview here — a cut marks a receipt boundary
            // a POS relies on to separate jobs — but record that the cut did not
            // happen, so callers can surface it rather than mistake the split for
            // a real cut.
            self.warnings.push(RenderWarning::UncuttableCut {
                command: if partial {
                    "GS V partial cut"
                } else {
                    "GS V full cut"
                },
                profile: self.profile_id.clone(),
                offset,
            });
        }

        let sheet_count = self.completed_sheets.len().saturating_add(1);
        if sheet_count > self.limits.max_sheets {
            return Err(RenderError::LimitExceeded {
                kind: LimitKind::Sheets,
                value: sheet_count as u64,
                limit: self.limits.max_sheets as u64,
            });
        }

        // Function A cuts at the current paper position; it does not add a
        // model-dependent feed-to-cutter distance.
        let next_roll = self.roll.fork(self.roll.width());
        self.completed_sheets
            .push(std::mem::replace(&mut self.roll, next_roll));
        self.line_top = 0;
        Ok(())
    }

    pub(crate) fn require_column_bit_image(&self, offset: usize) -> Result<(), RenderError> {
        self.require_profile_feature(
            self.supports_column_bit_image,
            "ESC * column bit image",
            offset,
        )
    }

    pub(crate) fn require_raster_bit_image(&self, offset: usize) -> Result<(), RenderError> {
        self.require_profile_feature(
            self.supports_raster_bit_image,
            "GS v 0 raster bit image",
            offset,
        )
    }

    pub(crate) fn require_graphics(&self, offset: usize) -> Result<(), RenderError> {
        self.require_profile_feature(self.supports_graphics, "GS ( L graphics", offset)
    }

    pub(crate) fn require_barcode_system(
        &self,
        system: BarcodeSystem,
        is_function_a: bool,
        offset: usize,
    ) -> Result<(), RenderError> {
        let supported = if is_function_a {
            self.function_a_barcodes.contains(&system)
        } else {
            self.function_b_barcodes.contains(&system)
        };
        self.require_profile_feature(supported, barcode_system_command_name(system), offset)
    }

    pub(crate) fn require_qr(&self, offset: usize) -> Result<(), RenderError> {
        self.require_profile_feature(self.supports_qr, "GS ( k QR code", offset)
    }

    pub(crate) fn require_profile_feature(
        &self,
        supported: bool,
        command: &'static str,
        offset: usize,
    ) -> Result<(), RenderError> {
        if supported {
            return Ok(());
        }

        Err(RenderError::CommandUnsupportedByProfile {
            command,
            profile: self.profile_id.clone(),
            offset,
        })
    }

    pub(crate) fn validate_command_payload_size(
        &self,
        payload_bytes: usize,
    ) -> Result<(), RenderError> {
        if payload_bytes > self.limits.max_command_payload_bytes {
            return Err(RenderError::LimitExceeded {
                kind: LimitKind::CommandPayloadBytes,
                value: payload_bytes as u64,
                limit: self.limits.max_command_payload_bytes as u64,
            });
        }
        Ok(())
    }

    pub(crate) fn validate_roll_height(&self, height_dots: u32) -> Result<(), RenderError> {
        if height_dots > self.limits.max_sheet_height_dots {
            return Err(RenderError::LimitExceeded {
                kind: LimitKind::SheetHeightDots,
                value: u64::from(height_dots),
                limit: u64::from(self.limits.max_sheet_height_dots),
            });
        }

        let completed_dots = self
            .completed_sheets
            .iter()
            .map(|sheet| u64::from(sheet.width()) * u64::from(sheet.height()))
            .sum::<u64>();
        let current_dots = u64::from(self.roll.width()) * u64::from(height_dots);
        let total_dots = completed_dots.saturating_add(current_dots);
        if total_dots > self.limits.max_total_dots {
            return Err(RenderError::LimitExceeded {
                kind: LimitKind::TotalDots,
                value: total_dots,
                limit: self.limits.max_total_dots,
            });
        }
        Ok(())
    }

    pub(crate) fn drawer_pulse(
        &mut self,
        connector: u8,
        on_time_units: u8,
        off_time_units: u8,
        offset: usize,
    ) -> Result<(), RenderError> {
        if !matches!(connector, 0 | 1 | 48 | 49) {
            return Err(RenderError::UnsupportedDrawerConnector { connector, offset });
        }
        if !self.supports_standard_drawer_pulse {
            return Err(RenderError::CommandUnsupportedByProfile {
                command: "ESC p drawer pulse",
                profile: self.profile_id.clone(),
                offset,
            });
        }

        // Pulse timing affects the connector only; retain it as an event
        // without inventing any paper-side marks.
        self.device_events.push(DeviceEvent::CashDrawerPulse {
            connector,
            on_time_units,
            off_time_units,
        });
        Ok(())
    }

    pub(crate) fn mark_gs_v_0_printed(&mut self) {
        self.pending_gs_v_0_lf = true;
    }

    pub(crate) fn clear_pending_gs_v_0_lf(&mut self) {
        self.pending_gs_v_0_lf = false;
    }

    pub(crate) fn line_feed(&mut self) -> Result<(), RenderError> {
        if std::mem::take(&mut self.pending_gs_v_0_lf)
            && self.gs_v_0_following_lf_behavior == FeedBehavior::Ignored
        {
            return Ok(());
        }
        self.feed_lines(1)
    }

    pub(crate) fn carriage_return(&mut self) -> Result<(), RenderError> {
        match self.carriage_return_mode {
            CarriageReturnMode::Ignored => Ok(()),
            CarriageReturnMode::LineFeed => self.line_feed(),
        }
    }

    pub(crate) fn into_surfaces(mut self) -> Result<Vec<S>, RenderError> {
        // A cut already finalized the preceding roll. Do not invent a blank
        // trailing receipt when the job ends immediately after that cut.
        if self.roll.height() > 0 {
            let sheet_count = self.completed_sheets.len().saturating_add(1);
            if sheet_count > self.limits.max_sheets {
                return Err(RenderError::LimitExceeded {
                    kind: LimitKind::Sheets,
                    value: sheet_count as u64,
                    limit: self.limits.max_sheets as u64,
                });
            }
            self.completed_sheets.push(self.roll);
        }
        Ok(self.completed_sheets)
    }
}
