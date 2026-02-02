use anyhow::Result;
use base64::Engine;
use image::{DynamicImage, GenericImageView};
use std::io::Write;

/// Kitty Graphics Protocol implementation
pub struct KittyProtocol;

impl KittyProtocol {
    /// Encode and send an image using KGP
    pub fn encode_image(
        img: &DynamicImage,
        writer: &mut dyn Write,
        cols: u16,
        rows: u16,
    ) -> Result<()> {
        let (orig_width, orig_height) = img.dimensions();

        // Resize to fit within cell dimensions
        let (font_width, font_height) = (8, 16); // TODO: get from terminal
        let max_pixel_width = cols as u32 * font_width;
        let max_pixel_height = rows as u32 * font_height;

        let resized = if orig_width > max_pixel_width || orig_height > max_pixel_height {
            img.resize(
                max_pixel_width,
                max_pixel_height,
                image::imageops::FilterType::Lanczos3,
            )
        } else {
            img.clone()
        };

        let (width, height) = resized.dimensions();

        // Convert to RGBA
        let rgba = resized.to_rgba8();
        let raw_data = rgba.as_raw();

        // Encode as base64
        let b64_data = base64::engine::general_purpose::STANDARD.encode(raw_data);

        // Build KGP escape sequence
        // Format: <ESC>_Ga=T,f=32,s=<w>,v=<h>,m=1;<data><ESC>\
        let chunks: Vec<&str> = b64_data
            .as_bytes()
            .chunks(4096)
            .map(|chunk| std::str::from_utf8(chunk).unwrap())
            .collect();

        let num_chunks = chunks.len();

        for (i, chunk) in chunks.iter().enumerate() {
            let more = if i < num_chunks - 1 { 1 } else { 0 };
            let f = 32; // RGBA format

            write!(
                writer,
                "\x1b_Ga=T,f={},s={},v={},m={};{}\x1b\\",
                f, width, height, more, chunk
            )?;
        }

        // Add Unicode placeholders to reserve space
        // Each placeholder character represents one cell
        for row in 0..rows {
            for col in 0..cols {
                // Use U+10EEEE as placeholder (Kitty extension)
                write!(writer, "\u{10EEEE}")?;
            }
            if row < rows - 1 {
                write!(writer, "\n")?;
            }
        }

        writer.flush()?;
        Ok(())
    }

    /// Clear/hide an image at given position
    pub fn clear_image(writer: &mut dyn Write) -> Result<()> {
        // Send delete command
        write!(writer, "\x1b_Ga=d;\x1b\\")?;
        writer.flush()?;
        Ok(())
    }
}
