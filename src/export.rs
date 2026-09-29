use image::{
    codecs::gif::{GifEncoder, Repeat},
    Delay, Frame, RgbaImage,
};
use std::{fs::File, io::BufWriter, path::Path};

pub fn png(path: &Path, rgba: &[u8]) -> Result<(), String> {
    image::save_buffer_with_format(
        path,
        rgba,
        32,
        32,
        image::ColorType::Rgba8,
        image::ImageFormat::Png,
    )
    .map_err(|e| e.to_string())
}

pub fn gif(path: &Path, frames: &[Vec<u8>], speed: u16) -> Result<(), String> {
    if frames.is_empty() {
        return Err("This file has no color icon".into());
    }
    let file = File::create(path).map_err(|e| e.to_string())?;
    let mut encoder = GifEncoder::new(BufWriter::new(file));
    encoder
        .set_repeat(Repeat::Infinite)
        .map_err(|e| e.to_string())?;
    // Stored duration uses 1/30-second units (Sega VMU Tutorial VMT-18).
    for rgba in frames {
        let image = RgbaImage::from_raw(32, 32, rgba.clone()).ok_or("Invalid icon dimensions")?;
        let frame = Frame::from_parts(
            image,
            0,
            0,
            Delay::from_numer_denom_ms(u32::from(speed.max(1)) * 1000, 30),
        );
        encoder.encode_frame(frame).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{AnimationDecoder, ImageReader};
    #[test]
    fn exports_original_pixels_and_animation() {
        let temp = tempfile::tempdir().unwrap();
        let red = [255, 0, 0, 255].repeat(1024);
        let blue = [0, 0, 255, 255].repeat(1024);
        let png_path = temp.path().join("icon.png");
        png(&png_path, &red).unwrap();
        assert_eq!(
            ImageReader::open(png_path)
                .unwrap()
                .decode()
                .unwrap()
                .to_rgba8()
                .into_raw(),
            red
        );
        let gif_path = temp.path().join("icon.gif");
        gif(&gif_path, &[red.clone(), blue.clone()], 15).unwrap();
        let decoder = image::codecs::gif::GifDecoder::new(std::io::BufReader::new(
            File::open(gif_path).unwrap(),
        ))
        .unwrap();
        let frames = decoder.into_frames().collect_frames().unwrap();
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].buffer().as_raw(), &red);
        assert_eq!(frames[1].buffer().as_raw(), &blue);
        assert_eq!(frames[0].delay().numer_denom_ms(), (500, 1));
    }
}
