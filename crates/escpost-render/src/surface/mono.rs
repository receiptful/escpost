//! Monochrome dot surface and its PNG encoding.
//!
//! The surface stores 8-bit coverage per subpixel at `scale ×` the dot
//! resolution (255 = full ink). Two independent knobs shape the output:
//!
//! - `scale` is the pixel density (subpixels per dot).
//! - `antialias` selects the encoding. Without it the surface is the faithful
//!   1-bit dot grid — glyph coverage is thresholded to 0/255 by the caller, so
//!   it packs to a 1-bit PNG, the authority for golden tests and for what the
//!   printer actually prints. With it, glyphs keep their intermediate coverage
//!   and encode as an 8-bit grayscale preview.
//!
//! Dot-space drawing (`print_dot`, barcodes, bitmaps, reverse fills, underlines)
//! always fills hard `scale × scale` blocks; only glyphs write soft coverage.

use super::RenderSurface;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonoSurface {
    pub(crate) width: u32,  // dots
    pub(crate) height: u32, // dots
    scale: u32,
    antialias: bool,
    // Row-major coverage, one byte per subpixel, `width * scale` per row.
    coverage: Vec<u8>,
}

/// Subpixel coverage at or above which a dot reads as printed and packs to a set
/// bit in the 1-bit encoding.
const INK_THRESHOLD: u8 = 128;

impl MonoSurface {
    #[must_use]
    pub fn width(&self) -> u32 {
        self.width
    }

    #[must_use]
    pub fn height(&self) -> u32 {
        self.height
    }

    #[must_use]
    pub fn is_printed(&self, x: u32, y: u32) -> bool {
        if x >= self.width || y >= self.height {
            return false;
        }
        let half = self.scale / 2;
        let sx = x * self.scale + half;
        let sy = y * self.scale + half;
        self.coverage[(sy * self.stride() + sx) as usize] >= INK_THRESHOLD
    }

    fn stride(&self) -> u32 {
        self.width * self.scale
    }
}

impl RenderSurface for MonoSurface {
    fn new(width: u32, scale: u32, antialias: bool) -> Self {
        Self::new(width, scale, antialias)
    }

    fn fork(&self, width: u32) -> Self {
        Self::new(width, self.scale, self.antialias)
    }

    fn width(&self) -> u32 {
        self.width
    }

    fn height(&self) -> u32 {
        self.height
    }

    fn print_dot(&mut self, x: u32, y: u32) {
        self.print_dot(x, y);
    }

    fn blend_subpixel(&mut self, sx: u32, sy: u32, value: u8, add: bool) {
        self.blend_subpixel(sx, sy, value, add);
    }

    fn composite_at(&mut self, source: &Self, left: u32, top: u32) {
        self.composite_at(source, left, top);
    }

    fn ensure_height(&mut self, height: u32) {
        self.ensure_height(height);
    }

    fn clear(&mut self) {
        self.clear();
    }
}

impl MonoSurface {
    pub(crate) fn new(width: u32, scale: u32, antialias: bool) -> Self {
        Self {
            width,
            height: 0,
            scale,
            antialias,
            coverage: Vec::new(),
        }
    }

    pub(crate) fn print_dot(&mut self, x: u32, y: u32) {
        if x >= self.width {
            return;
        }
        self.ensure_height(y + 1);
        self.fill_block(x, y, 255);
    }

    fn fill_block(&mut self, x: u32, y: u32, value: u8) {
        let stride = self.stride();
        for sy in y * self.scale..(y + 1) * self.scale {
            for sx in x * self.scale..(x + 1) * self.scale {
                self.coverage[(sy * stride + sx) as usize] = value;
            }
        }
    }

    /// Blend a glyph subpixel. `add` maxes the coverage (ink over paper);
    /// otherwise it subtracts (carving paper out of a reverse block). The caller
    /// has reserved the height; out-of-bounds subpixels are dropped.
    pub(crate) fn blend_subpixel(&mut self, sx: u32, sy: u32, value: u8, add: bool) {
        let stride = self.stride();
        if sx >= stride || sy >= self.height * self.scale {
            return;
        }
        let index = (sy * stride + sx) as usize;
        self.coverage[index] = if add {
            self.coverage[index].max(value)
        } else {
            self.coverage[index].saturating_sub(value)
        };
    }

    pub(crate) fn composite_at(&mut self, source: &Self, left: u32, top: u32) {
        if source.height == 0 {
            return;
        }
        self.ensure_height(top.saturating_add(source.height));
        let stride = self.stride();
        let source_stride = source.stride();
        for sy in 0..source.height * source.scale {
            let dy = top * self.scale + sy;
            for sx in 0..source.width * source.scale {
                let value = source.coverage[(sy * source_stride + sx) as usize];
                if value == 0 {
                    continue;
                }
                let dx = left * self.scale + sx;
                if dx >= stride || dy >= self.height * self.scale {
                    continue;
                }
                let index = (dy * stride + dx) as usize;
                self.coverage[index] = self.coverage[index].max(value);
            }
        }
    }

    pub(crate) fn ensure_height(&mut self, height: u32) {
        if height <= self.height {
            return;
        }
        let stride = self.stride();
        self.coverage
            .resize((height * self.scale * stride) as usize, 0);
        self.height = height;
    }

    pub(crate) fn clear(&mut self) {
        self.height = 0;
        self.coverage.clear();
    }
}

pub(crate) fn encode_png(surface: &MonoSurface) -> Result<Vec<u8>, png::EncodingError> {
    if surface.antialias {
        encode_grayscale(surface)
    } else {
        encode_bilevel(surface)
    }
}

fn encode_bilevel(surface: &MonoSurface) -> Result<Vec<u8>, png::EncodingError> {
    // Pack coverage (0/255 in the faithful path) into PNG's 1-bit layout:
    // bit 1 = white (paper), bit 0 = black (ink), so a subpixel at or above the
    // ink threshold clears its bit.
    let width = surface.width * surface.scale;
    let height = surface.height * surface.scale;
    let row_bytes = width.div_ceil(8) as usize;
    let mut pixels = vec![0u8; row_bytes * height as usize];
    for y in 0..height as usize {
        for x in 0..width as usize {
            if surface.coverage[y * width as usize + x] < INK_THRESHOLD {
                pixels[y * row_bytes + x / 8] |= 0x80 >> (x % 8);
            }
        }
    }

    let mut encoded = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut encoded, width, height);
        encoder.set_color(png::ColorType::Grayscale);
        encoder.set_depth(png::BitDepth::One);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(&pixels)?;
    }
    Ok(encoded)
}

fn encode_grayscale(surface: &MonoSurface) -> Result<Vec<u8>, png::EncodingError> {
    // Coverage is ink intensity (255 = ink); PNG grayscale is luminance
    // (0 = black), so invert each subpixel.
    let pixels: Vec<u8> = surface.coverage.iter().map(|value| 255 - value).collect();

    let mut encoded = Vec::new();
    {
        let mut encoder = png::Encoder::new(
            &mut encoded,
            surface.width * surface.scale,
            surface.height * surface.scale,
        );
        encoder.set_color(png::ColorType::Grayscale);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(&pixels)?;
    }
    Ok(encoded)
}
