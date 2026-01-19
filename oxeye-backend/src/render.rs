//! Image rendering for Minecraft skins.
//!
//! This module handles:
//! - Rendering player head images (64x64) from skin PNGs
//! - Generating composite status images with multiple player heads

use ab_glyph::{Font, FontRef, PxScale};
use image::{DynamicImage, GenericImageView, ImageFormat, Rgba, RgbaImage, imageops};
use imageproc::drawing::draw_text_mut;
use std::cmp::min;
use std::io::Cursor;

/// Default Steve head image (embedded at compile time).
pub const DEFAULT_STEVE_HEAD: &[u8] = include_bytes!("../assets/steve_head.png");

/// Inter font for rendering usernames (embedded at compile time).
const INTER_FONT: &[u8] = include_bytes!("../assets/Inter.ttf");

/// Render a 64x64 head image from a Minecraft skin PNG.
///
/// The head is composed of:
/// - Face layer: 8x8 at position (8, 8)
/// - Helmet overlay: 8x8 at position (40, 8)
///
/// The two layers are composited and scaled to 64x64 using nearest-neighbor
/// interpolation (to preserve the pixelated Minecraft style).
pub fn render_head(skin_png: &[u8]) -> Result<Vec<u8>, RenderError> {
    // Load the skin image
    let skin =
        image::load_from_memory(skin_png).map_err(|e| RenderError::ImageLoad(e.to_string()))?;

    // Verify skin dimensions (should be 64x64 or 64x32 for old format)
    let (width, height) = skin.dimensions();
    if width != 64 || (height != 64 && height != 32) {
        return Err(RenderError::InvalidSkinDimensions { width, height });
    }

    // Crop face (8x8 at position 8,8)
    let face = skin.crop_imm(8, 8, 8, 8).to_rgba8();

    // Crop helmet overlay (8x8 at position 40,8)
    // Only available in new skin format (64x64)
    let mut head = face;
    if height == 64 {
        let helmet = skin.crop_imm(40, 8, 8, 8).to_rgba8();
        // Composite helmet over face (respecting alpha)
        imageops::overlay(&mut head, &helmet, 0, 0);
    }

    // Scale to 64x64 with nearest-neighbor (pixelated look)
    let head = imageops::resize(&head, 64, 64, imageops::FilterType::Nearest);

    // Encode to PNG
    let mut buf = Vec::new();
    head.write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)
        .map_err(|e| RenderError::ImageEncode(e.to_string()))?;

    Ok(buf)
}

/// Configuration for composite image rendering.
pub struct CompositeConfig {
    /// Size of each head image
    pub head_size: u32,
    /// Horizontal spacing between heads
    pub h_spacing: u32,
    /// Vertical spacing between rows
    pub v_spacing: u32,
    /// Height reserved for username text below each head
    pub text_height: u32,
    /// Maximum number of players per row
    pub max_per_row: usize,
    /// Base font size for usernames
    pub font_size: f32,
    /// Minimum font size when scaling for long names
    pub min_font_size: f32,
}

impl Default for CompositeConfig {
    fn default() -> Self {
        Self {
            head_size: 64,
            h_spacing: 16,
            v_spacing: 8,
            text_height: 24,
            max_per_row: 5,
            font_size: 16.0,
            min_font_size: 10.0,
        }
    }
}

/// A player entry for composite rendering.
pub struct PlayerEntry {
    pub name: String,
    /// Pre-rendered head image (64x64 PNG), or None for Steve fallback
    pub head_data: Option<Vec<u8>>,
}

/// Render a composite status image showing multiple player heads in a grid.
///
/// Layout:
/// - Maximum 5 players per row
/// - Rows are center-aligned
/// - Each cell contains a 64x64 head with the username below
/// - Transparent background
///
/// Returns "No players online" text if the player list is empty.
pub fn render_composite(
    players: &[PlayerEntry],
    config: &CompositeConfig,
) -> Result<Vec<u8>, RenderError> {
    // Load the font
    let font =
        FontRef::try_from_slice(INTER_FONT).map_err(|e| RenderError::FontLoad(e.to_string()))?;

    // Handle empty state
    if players.is_empty() {
        return render_empty_state(&font, config);
    }

    // Calculate dimensions
    let num_rows = (players.len() + config.max_per_row - 1) / config.max_per_row;
    let cell_height = config.head_size + config.text_height + config.v_spacing;

    // Max width: 5 heads with spacing
    let max_width = (config.head_size * config.max_per_row as u32)
        + (config.h_spacing * (config.max_per_row as u32 - 1));
    let height = cell_height * num_rows as u32;

    // Create transparent canvas
    let mut canvas = RgbaImage::from_pixel(max_width, height, Rgba([0, 0, 0, 0]));

    // Load Steve head fallback
    let steve_head = image::load_from_memory(DEFAULT_STEVE_HEAD)
        .map_err(|e| RenderError::ImageLoad(e.to_string()))?
        .to_rgba8();

    // Draw each player
    for (i, player) in players.iter().enumerate() {
        let row = i / config.max_per_row;
        let col = i % config.max_per_row;
        let items_in_row = min(config.max_per_row, players.len() - row * config.max_per_row);

        // Calculate row width for centering
        let row_width = (config.head_size * items_in_row as u32)
            + (config.h_spacing * (items_in_row as u32).saturating_sub(1));
        let x_offset = (max_width - row_width) / 2;

        let x = x_offset + (col as u32) * (config.head_size + config.h_spacing);
        let y = (row as u32) * cell_height;

        // Load and draw head
        let head = if let Some(head_data) = &player.head_data {
            image::load_from_memory(head_data)
                .map(|img| img.to_rgba8())
                .unwrap_or_else(|_| steve_head.clone())
        } else {
            steve_head.clone()
        };

        // Resize if needed (should already be 64x64, but just in case)
        let head = if head.dimensions() != (config.head_size, config.head_size) {
            imageops::resize(
                &head,
                config.head_size,
                config.head_size,
                imageops::FilterType::Nearest,
            )
        } else {
            head
        };

        imageops::overlay(&mut canvas, &head, x.into(), y.into());

        // Calculate font size (scale down for long names)
        let font_size = calculate_font_size(&player.name, config);
        let scale = PxScale::from(font_size);

        // Measure text width for centering
        let text_width = measure_text_width(&font, &player.name, scale);
        let text_x = x + (config.head_size / 2) - (text_width / 2);
        let text_y = y + config.head_size + 4;

        // Draw username (white text)
        draw_text_mut(
            &mut canvas,
            Rgba([255, 255, 255, 255]),
            text_x as i32,
            text_y as i32,
            scale,
            &font,
            &player.name,
        );
    }

    // Encode to PNG
    let mut buf = Vec::new();
    DynamicImage::ImageRgba8(canvas)
        .write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)
        .map_err(|e| RenderError::ImageEncode(e.to_string()))?;

    Ok(buf)
}

/// Render the empty state image ("No players online").
fn render_empty_state(
    font: &FontRef<'_>,
    config: &CompositeConfig,
) -> Result<Vec<u8>, RenderError> {
    let text = "No players online";
    let scale = PxScale::from(config.font_size);

    // Measure text
    let text_width = measure_text_width(font, text, scale);
    let padding = 20u32;
    let width = text_width + padding * 2;
    let height = config.font_size as u32 + padding * 2;

    // Create transparent canvas
    let mut canvas = RgbaImage::from_pixel(width, height, Rgba([0, 0, 0, 0]));

    // Draw centered text
    let x = padding;
    let y = padding;

    draw_text_mut(
        &mut canvas,
        Rgba([180, 180, 180, 255]), // Light gray
        x as i32,
        y as i32,
        scale,
        font,
        text,
    );

    // Encode to PNG
    let mut buf = Vec::new();
    DynamicImage::ImageRgba8(canvas)
        .write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)
        .map_err(|e| RenderError::ImageEncode(e.to_string()))?;

    Ok(buf)
}

/// Calculate font size for a username, scaling down for long names.
fn calculate_font_size(name: &str, config: &CompositeConfig) -> f32 {
    // Approximate: each character is about 0.6 * font_size wide for Inter
    let char_width_ratio = 0.6;
    let max_text_width = config.head_size as f32;
    let estimated_width = name.len() as f32 * config.font_size * char_width_ratio;

    if estimated_width <= max_text_width {
        config.font_size
    } else {
        // Scale down proportionally
        let scale_factor = max_text_width / estimated_width;
        (config.font_size * scale_factor).max(config.min_font_size)
    }
}

/// Measure the width of text in pixels.
fn measure_text_width(font: &FontRef<'_>, text: &str, scale: PxScale) -> u32 {
    let mut width = 0.0f32;
    let scale_factor = scale.x / font.height_unscaled();
    for c in text.chars() {
        let glyph_id = font.glyph_id(c);
        let advance = font.h_advance_unscaled(glyph_id);
        width += advance * scale_factor;
    }
    width as u32
}

/// Errors that can occur during rendering.
#[derive(Debug)]
pub enum RenderError {
    /// Failed to load image from memory
    ImageLoad(String),
    /// Invalid skin dimensions
    InvalidSkinDimensions { width: u32, height: u32 },
    /// Failed to encode image
    ImageEncode(String),
    /// Failed to load font
    FontLoad(String),
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RenderError::ImageLoad(e) => write!(f, "failed to load image: {}", e),
            RenderError::InvalidSkinDimensions { width, height } => {
                write!(
                    f,
                    "invalid skin dimensions: {}x{} (expected 64x64 or 64x32)",
                    width, height
                )
            }
            RenderError::ImageEncode(e) => write!(f, "failed to encode image: {}", e),
            RenderError::FontLoad(e) => write!(f, "failed to load font: {}", e),
        }
    }
}

impl std::error::Error for RenderError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_head_with_fallback() {
        // Test that render_head returns an error for invalid data
        let invalid_skin = b"not a valid png";
        let result = render_head(invalid_skin);
        assert!(result.is_err());
    }

    #[test]
    fn test_render_head_from_steve_head() {
        // Use the embedded Steve head as a "skin" - it will fail dimension check
        // but this tests the error path for wrong dimensions
        let result = render_head(DEFAULT_STEVE_HEAD);
        // Steve head is 64x64 but it's a head, not a skin (skin should be 64x64 or 64x32)
        // This should actually work since dimensions are 64x64
        // If it fails, it would be due to it being a head not a full skin
        assert!(result.is_ok());
    }

    #[test]
    fn test_render_empty_composite() {
        let config = CompositeConfig::default();
        let result = render_composite(&[], &config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_render_composite_single_player() {
        let config = CompositeConfig::default();
        let players = vec![PlayerEntry {
            name: "Steve".to_string(),
            head_data: None, // Uses Steve fallback
        }];
        let result = render_composite(&players, &config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_render_composite_multiple_players() {
        let config = CompositeConfig::default();
        let players = vec![
            PlayerEntry {
                name: "Steve".to_string(),
                head_data: None,
            },
            PlayerEntry {
                name: "Alex".to_string(),
                head_data: None,
            },
            PlayerEntry {
                name: "Notch".to_string(),
                head_data: None,
            },
            PlayerEntry {
                name: "jeb_".to_string(),
                head_data: None,
            },
            PlayerEntry {
                name: "Dinnerbone".to_string(),
                head_data: None,
            },
            PlayerEntry {
                name: "Grumm".to_string(),
                head_data: None,
            },
            PlayerEntry {
                name: "LongUsernamePerson".to_string(),
                head_data: None,
            },
        ];
        let result = render_composite(&players, &config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_font_size_scaling() {
        let config = CompositeConfig::default();

        // Short name should use full font size
        let size = calculate_font_size("Steve", &config);
        assert!((size - config.font_size).abs() < 0.01);

        // Long name should scale down
        let size = calculate_font_size("VeryLongUsername123", &config);
        assert!(size < config.font_size);
        assert!(size >= config.min_font_size);
    }
}
