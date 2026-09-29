//! Composite the supplied Flycast body/shading plates with unfiltered VMU pixels.
use crate::vmu::root::{self, Settings};
use image::{imageops, Rgba, RgbaImage};
use std::sync::OnceLock;

struct Plates {
    body: RgbaImage,
    buttons: RgbaImage,
    screen: [u32; 3], // x, y, square width in the source plate
}
fn plates(real: bool) -> &'static Plates {
    static NORMAL: OnceLock<Plates> = OnceLock::new();
    static REAL: OnceLock<Plates> = OnceLock::new();
    let decode = |bytes: &[u8]| {
        image::load_from_memory(bytes)
            .expect("Flycast plate")
            .to_rgba8()
    };
    if real {
        REAL.get_or_init(|| Plates {
            body: decode(include_bytes!(
                "../assets/real_mode_color_alpha_template.png"
            )),
            buttons: decode(include_bytes!("../assets/real_mode_buttons.png")),
            screen: [82, 158, 384],
        })
    } else {
        NORMAL.get_or_init(|| Plates {
            body: decode(include_bytes!(
                "../assets/normal_mode_color_alpha_template.png"
            )),
            buttons: decode(include_bytes!("../assets/normal_mode_buttons.png")),
            screen: [61, 105, 512],
        })
    }
}

pub fn pixel_scale(real: bool, width: f32, height: f32) -> u32 {
    let p = plates(real);
    let factor = (width / p.body.width() as f32).min(height / p.body.height() as f32);
    (factor * p.screen[2] as f32 / 32.0)
        .floor()
        .clamp(1.0, 12.0) as u32
}

pub fn render(settings: &Settings, scale: u32) -> RgbaImage {
    let p = plates(settings.real_mode);
    let rgba = if settings.custom_color {
        settings.rgba
    } else {
        [255; 4]
    };
    let color_icon = settings.color.as_ref();
    // Always use the filled, buttonless plate. There is no filtered screen
    // cutout to expose a fractional strip around the integer-sized icon.
    let mut body = p.body.clone();
    let mut buttons = p.buttons.clone();
    for (x, y, button) in buttons.enumerate_pixels_mut() {
        // The normal export includes stray silhouette lines outside the controls.
        // Keep the complete control region, including nearly-black Phong shadows.
        if !settings.real_mode && !(50..580).contains(&x)
            || !settings.real_mode && !(620..885).contains(&y)
        {
            *button = Rgba([0; 4]);
        }
    }
    for pixel in body.pixels_mut() {
        pixel[3] = (u32::from(pixel[3]) * 255 / 140).min(255) as u8;
        for c in 0..3 {
            pixel[c] = ((u16::from(pixel[c]) * u16::from(rgba[c]) + 127) / 255) as u8;
        }
        pixel[3] = ((u16::from(pixel[3]) * u16::from(rgba[3]) + 127) / 255) as u8;
    }
    // Composite at source resolution, then filter once. Separately filtering
    // complementary button/body edges creates pale rings at the shared boundary.
    imageops::overlay(&mut body, &buttons, 0, 0);
    let side = 32 * scale.max(1);
    let ratio = side as f32 / p.screen[2] as f32;
    let width = (p.body.width() as f32 * ratio).round() as u32;
    let height = (p.body.height() as f32 * ratio).round() as u32;
    let mut result = resize_layer(&body, width, height);
    let mono = settings
        .mono
        .unwrap_or_else(|| root::bios_mono(settings.shape));
    let icon = if let Some(icon) = color_icon {
        icon.rgba()
    } else {
        root::bios_mono_rgba(&mono)
    };
    let icon = RgbaImage::from_raw(32, 32, icon).unwrap();
    let icon = imageops::resize(&icon, side, side, imageops::FilterType::Nearest);
    imageops::overlay(
        &mut result,
        &icon,
        (p.screen[0] as f32 * ratio).round() as i64,
        (p.screen[1] as f32 * ratio).round() as i64,
    );
    result
}

// Filter premultiplied channels so transparent plate edges cannot acquire halos.
fn resize_layer(source: &RgbaImage, width: u32, height: u32) -> RgbaImage {
    let mut premultiplied = source.clone();
    for p in premultiplied.pixels_mut() {
        for c in 0..3 {
            p[c] = (u16::from(p[c]) * u16::from(p[3]) / 255) as u8;
        }
    }
    let mut result = imageops::resize(
        &premultiplied,
        width,
        height,
        imageops::FilterType::Lanczos3,
    );
    for p in result.pixels_mut() {
        if p[3] > 0 {
            for c in 0..3 {
                p[c] = (u32::from(p[c]) * 255 / u32::from(p[3])).min(255) as u8;
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn real_mode_keeps_every_opaque_button_shadow_pixel() {
        let mut settings = Settings::read(&root::empty_image());
        settings.real_mode = true;
        settings.custom_color = true;
        settings.rgba = [230, 60, 90, 100];
        // At scale 12 the Real Mode source and output are pixel-for-pixel.
        let image = render(&settings, 12);
        let buttons = &plates(true).buttons;
        assert_eq!(image.dimensions(), buttons.dimensions());
        let mut dark = 0;
        for (x, y, p) in buttons.enumerate_pixels() {
            if p[3] == 255 {
                assert_eq!(image.get_pixel(x, y), p, "button pixel {x},{y}");
                if p[2] <= p[0].saturating_add(5) {
                    dark += 1;
                }
            }
        }
        assert!(
            dark > 100,
            "exercise the formerly discarded nearly-black shadows"
        );
    }
    #[test]
    fn body_alpha_does_not_fade_buttons_or_mono_screen() {
        for real in [false, true] {
            let mut settings = Settings::read(&root::empty_image());
            settings.real_mode = real;
            settings.custom_color = true;
            settings.rgba = [230, 30, 50, 0];
            let image = render(&settings, 4);
            let p = plates(real);
            let ratio = 128.0 / p.screen[2] as f32;
            let x = (p.screen[0] as f32 * ratio).round() as u32;
            let y = (p.screen[1] as f32 * ratio).round() as u32;
            for iy in 0..128 {
                for ix in 0..128 {
                    let color = image.get_pixel(x + ix, y + iy).0;
                    assert!(color == [0xbb, 0xcc, 0x66, 255] || color == [0x22, 0x22, 0x55, 255]);
                }
            }
            assert_eq!(image.get_pixel(image.width() / 2, 10)[3], 0);
            assert!(image
                .enumerate_pixels()
                .any(|(_, y, p)| y > image.height() * 2 / 3 && p[3] == 255));
            settings.color = Some(root::ColorIcon {
                palette: [[0; 4]; 16],
                pixels: vec![0; 1024],
            });
            assert_eq!(render(&settings, 4).get_pixel(x + 64, y + 64)[3], 0);
            settings.rgba = [230, 30, 50, 255];
            let tinted = render(&settings, 4);
            let center = tinted.get_pixel(x + 64, y + 64);
            assert_eq!(center[3], 255);
            assert!(center[0] > center[2] && center[2] > center[1]);
        }
    }
}
