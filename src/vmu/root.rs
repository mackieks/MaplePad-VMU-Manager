//! Root metadata and ICONDATA_VMS are edited as one in-memory transaction.
//! Layout: KallistiOS vmu_root_t and Marcus Comstedt's ICONDATA_VMS notes.
use super::*;

// 124 monochrome BIOS icons, each 32 x 32 pixels at one bit per pixel.
const BIOS_ICONS: &[u8; 124 * 128] = include_bytes!("../../assets/dc_bios_icons.bin");
pub const REAL_MODE: [u8; 16] = [
    0xda, 0x69, 0xd0, 0xda, 0xc7, 0x4e, 0xf8, 0x36, 0x18, 0x92, 0x79, 0x68, 0x2d, 0xb5, 0x30, 0x86,
];

pub fn bios_mono(index: u16) -> [u8; 128] {
    let at = usize::from(if index <= 123 { index } else { 0 }) * 128;
    BIOS_ICONS[at..at + 128]
        .try_into()
        .expect("BIOS icon table")
}
pub fn mono_rgba(bits: &[u8; 128], color: [u8; 4]) -> Vec<u8> {
    (0..1024)
        .flat_map(|i| {
            if bits[i / 8] & (0x80 >> (i % 8)) != 0 {
                color
            } else {
                [0; 4]
            }
        })
        .collect()
}
pub fn bios_icon_pixel(active: bool) -> [u8; 4] {
    if active {
        [0x22, 0x22, 0x55, 255]
    } else {
        [0xbb, 0xcc, 0x66, 255]
    }
}

pub fn bios_mono_rgba(bits: &[u8; 128]) -> Vec<u8> {
    (0..1024)
        .flat_map(|i| bios_icon_pixel(bits[i / 8] & (0x80 >> (i % 8)) != 0))
        .collect()
}

#[derive(Clone, Debug, PartialEq)]
pub struct ColorIcon {
    pub palette: [[u8; 4]; 16],
    pub pixels: Vec<u8>, // 1024 unpacked palette indices, left to right.
}
impl ColorIcon {
    pub fn rgba(&self) -> Vec<u8> {
        self.pixels
            .iter()
            .flat_map(|&i| self.palette[usize::from(i)])
            .collect()
    }
    fn bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(544);
        for [r, g, b, a] in self.palette {
            let value = (u16::from(a / 17) << 12)
                | (u16::from(r / 17) << 8)
                | (u16::from(g / 17) << 4)
                | u16::from(b / 17);
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend(self.pixels.chunks_exact(2).map(|p| (p[0] << 4) | p[1]));
        bytes
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub capacity: u16,
    pub custom_color: bool,
    pub rgba: [u8; 4],
    pub shape: u16,
    pub real_mode: bool,
    pub color: Option<ColorIcon>,
    pub mono: Option<[u8; 128]>,
    pub description: String,
}
impl Settings {
    pub fn read(image: &VmuImage) -> Self {
        let root = &image.bytes[ROOT_OFFSET..];
        let icon = image.files.iter().find(|f| f.name == "ICONDATA_VMS");
        let color = icon.and_then(|f| {
            let offset = u32_at(&f.bytes, 0x14) as usize;
            let blob = f.bytes.get(offset..offset.checked_add(544)?)?;
            (offset >= 24).then(|| ColorIcon {
                palette: decode_palette(&blob[..32]),
                pixels: blob[32..].iter().flat_map(|b| [b >> 4, b & 15]).collect(),
            })
        });
        let mono = icon.and_then(|f| {
            let offset = u32_at(&f.bytes, 0x10) as usize;
            (offset >= 24).then_some(())?;
            f.bytes
                .get(offset..offset.checked_add(128)?)?
                .try_into()
                .ok()
        });
        Self {
            capacity: image.capacity as u16,
            custom_color: root[0x10] != 0,
            rgba: [root[0x13], root[0x12], root[0x11], root[0x14]],
            shape: if u16_at(root, 0x4e) <= 123 {
                u16_at(root, 0x4e)
            } else {
                0
            },
            real_mode: icon.is_some_and(|f| f.bytes.get(0x2c0..0x2d0) == Some(&REAL_MODE)),
            color,
            mono,
            description: icon.map_or_else(|| "Visual Memory".into(), |f| f.vm_description.clone()),
        }
    }
    pub fn icon_blocks(&self) -> usize {
        if self.color.is_some() || self.real_mode {
            2
        } else if self.mono.is_some() {
            1
        } else {
            0
        }
    }
}

pub fn apply(image: &VmuImage, settings: &Settings) -> Result<VmuImage, String> {
    if settings.shape > 123 {
        return Err("BIOS icon must be between 0 and 123.".into());
    }
    let old = Settings::read(image);
    let mut updated = image.clone();
    if !matches!(settings.capacity, 200 | 241) {
        return Err("Choose MaplePad 1.5 (200 blocks) or 2.0 (241 blocks).".into());
    }
    if usize::from(settings.capacity) != image.capacity {
        let bytes = if settings.capacity == 200 {
            prepare_for_target(&image.bytes, Firmware::MaplePad15)?
        } else {
            swap_words(&prepare_for_target(&image.bytes, Firmware::MaplePad20)?)
        };
        updated = parse_image(&bytes)?;
        updated.original_format = image.original_format.or(Some(image.format));
        updated.original_bytes = Some(
            image
                .original_bytes
                .as_ref()
                .unwrap_or(&image.bytes)
                .clone(),
        );
    }
    let icon_changed = old.color != settings.color
        || old.mono != settings.mono
        || old.real_mode != settings.real_mode
        || old.description != settings.description;
    if icon_changed {
        let existing = image.files.iter().position(|f| f.name == "ICONDATA_VMS");
        if settings.icon_blocks() == 0 {
            if let Some(index) = existing {
                updated = editing::delete(&updated, index)?;
            }
        } else {
            let (description, _, invalid) = SHIFT_JIS.encode(&settings.description);
            if invalid || description.len() > 16 {
                return Err("Icon description must fit in 16 Shift-JIS bytes.".into());
            }
            let mut bytes = vec![0; settings.icon_blocks() * BLOCK_SIZE];
            bytes[..description.len()].copy_from_slice(&description);
            bytes[0x10..0x14].copy_from_slice(&0x20u32.to_le_bytes());
            bytes[0x20..0xa0]
                .copy_from_slice(&settings.mono.unwrap_or_else(|| bios_mono(settings.shape)));
            if let Some(color) = &settings.color {
                if color.pixels.len() != 1024 || color.pixels.iter().any(|&i| i > 15) {
                    return Err("Invalid color icon pixels.".into());
                }
                bytes[0x14..0x18].copy_from_slice(&0xa0u32.to_le_bytes());
                bytes[0xa0..0x2c0].copy_from_slice(&color.bytes());
            }
            if settings.real_mode {
                bytes[0x2c0..0x2d0].copy_from_slice(&REAL_MODE);
            }
            let mut entry = [0u8; 32];
            entry[0] = 0x33;
            entry[4..16].copy_from_slice(b"ICONDATA_VMS");
            entry[16..24].copy_from_slice(&timestamp());
            entry[24..26].copy_from_slice(&(settings.icon_blocks() as u16).to_le_bytes());
            let file = parse_file(
                &entry,
                "ICONDATA_VMS".into(),
                0,
                settings.icon_blocks() as u16,
                0,
                bytes,
            );
            updated = if let Some(index) = existing {
                editing::replace(&updated, index, &file)?
            } else {
                editing::paste(&updated, &file, editing::Placement::FirstFit)?
            };
        }
    }
    let root = &mut updated.bytes[ROOT_OFFSET..];
    root[0x10] = u8::from(settings.custom_color);
    let [r, g, b, a] = settings.rgba;
    root[0x11..0x15].copy_from_slice(&[b, g, r, a]);
    root[0x4e..0x50].copy_from_slice(&settings.shape.to_le_bytes());
    if updated.bytes == image.bytes {
        return Ok(image.clone());
    }
    let mut parsed = parse_image(&updated.bytes)?;
    parsed.original_format = image.original_format.or(Some(image.format));
    if settings.capacity as usize == image.capacity {
        parsed.format = image.format;
    }
    parsed.original_bytes = Some(
        image
            .original_bytes
            .as_ref()
            .unwrap_or(&image.bytes)
            .clone(),
    );
    Ok(parsed)
}

pub fn empty_image() -> VmuImage {
    let mut bytes = vec![0; IMAGE_SIZE];
    let root = &mut bytes[ROOT_OFFSET..];
    root[..16].fill(0x55);
    root[0x11..0x15].fill(255);
    root[0x30..0x38].copy_from_slice(&timestamp());
    root[0x40..0x4c].copy_from_slice(&LEGACY_GEOMETRY);
    root[0x4c..0x4e].copy_from_slice(&13u16.to_le_bytes());
    root[0x50..0x52].copy_from_slice(&200u16.to_le_bytes());
    for block in 0..256 {
        let value: u16 = match block {
            0..=240 => FAT_FREE,
            241 | 254 | 255 => 0xfffa,
            _ => block as u16 - 1,
        };
        bytes[FAT_OFFSET + block * 2..FAT_OFFSET + block * 2 + 2]
            .copy_from_slice(&value.to_le_bytes());
    }
    parse_image(&bytes).expect("valid blank VMU")
}

pub fn timestamp() -> [u8; 8] {
    #[repr(C)]
    #[derive(Default)]
    struct SystemTime {
        year: u16,
        month: u16,
        day_of_week: u16,
        day: u16,
        hour: u16,
        minute: u16,
        second: u16,
        millis: u16,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetLocalTime(time: *mut SystemTime);
    }
    let mut time = SystemTime::default();
    unsafe {
        GetLocalTime(&mut time);
    }
    let bcd = |v: u16| ((v / 10) << 4 | v % 10) as u8;
    [
        bcd(time.year / 100),
        bcd(time.year % 100),
        bcd(time.month),
        bcd(time.day),
        bcd(time.hour),
        bcd(time.minute),
        bcd(time.second),
        bcd((time.day_of_week + 6) % 7),
    ]
}

/// Fit, rather than stretch, imported artwork onto the 32×32 icon canvas.
pub fn import_image(path: &std::path::Path) -> Result<Vec<u8>, String> {
    let reader = image::ImageReader::open(path).map_err(|e| e.to_string())?;
    let source = reader.decode().map_err(|e| e.to_string())?;
    let resized = source
        .resize(32, 32, image::imageops::FilterType::Lanczos3)
        .to_rgba8();
    let mut canvas = image::RgbaImage::new(32, 32);
    image::imageops::overlay(
        &mut canvas,
        &resized,
        i64::from((32 - resized.width()) / 2),
        i64::from((32 - resized.height()) / 2),
    );
    Ok(canvas.into_raw())
}
pub fn monochrome(rgba: &[u8], threshold: u8) -> [u8; 128] {
    let mut bits = [0; 128];
    for (i, p) in rgba.chunks_exact(4).take(1024).enumerate() {
        let luma = (u32::from(p[0]) * 299 + u32::from(p[1]) * 587 + u32::from(p[2]) * 114) / 1000;
        // Composite on white before thresholding; transparent padding stays off.
        let value = (luma * u32::from(p[3]) + 255 * (255 - u32::from(p[3]))) / 255;
        if value < u32::from(threshold) {
            bits[i / 8] |= 0x80 >> (i % 8);
        }
    }
    bits
}

/// Weighted median-cut in the hardware's ARGB4444 space; preserve exact small
/// palettes and reserve transparent black so transparent edges never turn solid.
pub fn optimize(rgba: &[u8]) -> ColorIcon {
    use std::collections::BTreeMap;
    let round = |c: u8| ((u16::from(c) + 8) / 17 * 17).min(255) as u8;
    let mut histogram = BTreeMap::<[u8; 4], usize>::new();
    let pixels: Vec<[u8; 4]> = rgba
        .chunks_exact(4)
        .map(|p| {
            let c = [round(p[0]), round(p[1]), round(p[2]), round(p[3])];
            if c[3] == 0 {
                [0; 4]
            } else {
                c
            }
        })
        .collect();
    for &c in &pixels {
        *histogram.entry(c).or_default() += 1;
    }
    let transparent = histogram.remove(&[0; 4]).is_some();
    let mut groups: Vec<Vec<([u8; 4], usize)>> = if histogram.is_empty() {
        vec![]
    } else {
        vec![histogram.into_iter().collect()]
    };
    while groups.len() < 16 - usize::from(transparent) {
        let candidate = groups
            .iter()
            .enumerate()
            .filter(|(_, g)| g.len() > 1)
            .map(|(i, g)| {
                let (axis, range) = (0..4)
                    .map(|a| {
                        (
                            a,
                            i32::from(g.iter().map(|(c, _)| c[a]).max().unwrap())
                                - i32::from(g.iter().map(|(c, _)| c[a]).min().unwrap()),
                        )
                    })
                    .max_by_key(|&(_, r)| r)
                    .unwrap();
                (
                    i,
                    axis,
                    range as usize * g.iter().map(|(_, n)| n).sum::<usize>(),
                )
            })
            .max_by_key(|&(_, _, score)| score);
        let Some((i, axis, _)) = candidate else {
            break;
        };
        let mut group = groups.remove(i);
        group.sort_by_key(|(c, _)| c[axis]);
        let half = group.iter().map(|(_, n)| n).sum::<usize>() / 2;
        let mut count = 0;
        let mut split = 1;
        for (at, (_, n)) in group.iter().enumerate() {
            count += n;
            split = (at + 1).clamp(1, group.len() - 1);
            if count >= half {
                break;
            }
        }
        let second = group.split_off(split);
        groups.push(group);
        groups.push(second);
    }
    let mut palette = [[0; 4]; 16];
    let offset = usize::from(transparent);
    let count = (groups.len() + offset).max(1);
    for (i, group) in groups.iter().enumerate() {
        let weight = group.iter().map(|(_, n)| n).sum::<usize>();
        for a in 0..4 {
            palette[i + offset][a] = round(
                (group
                    .iter()
                    .map(|(c, n)| usize::from(c[a]) * n)
                    .sum::<usize>()
                    / weight) as u8,
            );
        }
    }
    let distance = |a: [u8; 4], b: [u8; 4]| -> i64 {
        let mut d = 0;
        for channel in 0..3 {
            let diff =
                i64::from(a[channel]) * i64::from(a[3]) - i64::from(b[channel]) * i64::from(b[3]);
            d += diff * diff;
        }
        let alpha = (i64::from(a[3]) - i64::from(b[3])) * 255;
        d + 2 * alpha * alpha
    };
    let indices = pixels
        .iter()
        .map(|&p| (0..count).min_by_key(|&i| distance(p, palette[i])).unwrap() as u8)
        .collect();
    ColorIcon {
        palette,
        pixels: indices,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn card_mono_icons_use_bios_colors_independent_of_root_color() {
        let image = empty_image();
        let mut settings = Settings::read(&image);
        settings.shape = 31;
        let mut custom = [0_u8; 128];
        custom[0] = 0x80;
        for capacity in [200, 241] {
            settings.capacity = capacity;
            for mono in [None, Some(custom)] {
                settings.mono = mono;
                let bits = mono.unwrap_or_else(|| bios_mono(31));
                for color in [None, Some([255, 0, 0, 255]), Some([0, 255, 0, 0])] {
                    settings.custom_color = color.is_some();
                    settings.rgba = color.unwrap_or([255; 4]);
                    let updated = apply(&image, &settings).unwrap();
                    let mut formats = vec![updated.bytes.clone()];
                    if capacity == 241 {
                        formats.push(swap_words(&updated.bytes));
                    }
                    for bytes in formats {
                        let parsed = parse_image(&bytes).unwrap();
                        let rgba = parsed.card_icon_rgba.unwrap();
                        for (i, pixel) in rgba.chunks_exact(4).enumerate() {
                            let expected = if bits[i / 8] & (0x80 >> (i % 8)) != 0 {
                                [0x22, 0x22, 0x55, 255]
                            } else {
                                [0xbb, 0xcc, 0x66, 255]
                            };
                            assert_eq!(pixel, expected);
                        }
                        assert_eq!(parsed.card_color, color);
                    }
                }
            }
        }
        settings.color = Some(ColorIcon {
            palette: [[34, 136, 221, 102]; 16],
            pixels: vec![0; 1024],
        });
        let updated = apply(&image, &settings).unwrap();
        assert_eq!(
            updated.card_icon_rgba.unwrap(),
            settings.color.unwrap().rgba()
        );
    }

    #[test]
    fn new_image_has_valid_system_chains_and_default_bios_icon() {
        let image = empty_image();
        assert_eq!(
            (image.capacity, image.free, image.files.len()),
            (200, 200, 0)
        );
        assert_eq!(Settings::read(&image).shape, 0);
        for block in 242..=253 {
            assert_eq!(fat_entry(&image.bytes, block), block as u16 - 1);
        }
        for block in [241, 254, 255] {
            assert_eq!(fat_entry(&image.bytes, block), 0xfffa);
        }
        for index in 0..124 {
            assert!(bios_mono(index).iter().any(|&b| b != 0));
        }
        assert_eq!(
            apply(&image, &Settings::read(&image)).unwrap().bytes,
            image.bytes
        );
        assert!(apply(&image, &Settings::read(&image))
            .unwrap()
            .original_bytes
            .is_none());
        assert_ne!(
            decode_date(&image.bytes[ROOT_OFFSET + 0x30..ROOT_OFFSET + 0x38]),
            "—"
        );
    }
    #[test]
    fn root_edit_preserves_unknown_bytes_and_roundtrips_color_mono_and_real_mode() {
        let mut image = empty_image();
        image.bytes[ROOT_OFFSET + 0x90] = 0xa5;
        let mut settings = Settings::read(&image);
        settings.custom_color = true;
        settings.rgba = [204, 34, 51, 102];
        settings.shape = 31;
        let root_only = apply(&image, &settings).unwrap();
        assert_eq!(&root_only.bytes[..ROOT_OFFSET], &image.bytes[..ROOT_OFFSET]);
        assert_eq!(root_only.bytes[ROOT_OFFSET + 0x90], 0xa5);
        assert_eq!(root_only.card_color, Some(settings.rgba));
        let rgba: Vec<u8> = (0..1024)
            .flat_map(|i| {
                if i % 3 == 0 {
                    [0; 4]
                } else if i % 3 == 1 {
                    [255, 0, 0, 255]
                } else {
                    [0, 170, 255, 136]
                }
            })
            .collect();
        settings.color = Some(optimize(&rgba));
        settings.mono = Some(bios_mono(31));
        settings.real_mode = true;
        let updated = apply(&root_only, &settings).unwrap();
        assert_eq!(updated.free, 198);
        assert_eq!(Settings::read(&updated), settings);
        assert_eq!(updated.files[0].bytes[0x2c0..0x2d0], REAL_MODE);
        assert_eq!(updated.files[0].icon_frames[0], rgba);
        assert_eq!(updated.original_bytes.as_ref().unwrap(), &image.bytes);
        let first = updated.files[0].first_block;
        settings.real_mode = false;
        settings.color = None;
        let mono_only = apply(&updated, &settings).unwrap();
        assert_eq!(mono_only.free, 199);
        assert_eq!(mono_only.files[0].first_block, first);
        settings.mono = None;
        let removed = apply(&mono_only, &settings).unwrap();
        assert_eq!(removed.free, 200);
        assert!(removed.files.is_empty());
    }
    #[test]
    fn format_conversion_is_lossless_and_rejects_high_allocated_blocks() {
        let image = empty_image();
        let mut settings = Settings::read(&image);
        settings.capacity = 241;
        let upgraded = apply(&image, &settings).unwrap();
        assert_eq!(
            (upgraded.capacity, upgraded.free, upgraded.format),
            (241, 241, ImageFormat::Explorer20)
        );
        settings.capacity = 200;
        assert_eq!(apply(&upgraded, &settings).unwrap().bytes, image.bytes);
        let mut settings = Settings::read(&upgraded);
        settings.real_mode = true;
        let occupied = apply(&upgraded, &settings).unwrap();
        settings.capacity = 200;
        assert!(apply(&occupied, &settings)
            .unwrap_err()
            .contains("Block 239"));
        assert_eq!(occupied.capacity, 241);
    }
    #[test]
    fn palette_optimizer_and_mono_conversion_obey_hardware_limits() {
        let rgba: Vec<u8> = (0..1024)
            .flat_map(|i| {
                [
                    i as u8,
                    (i / 4) as u8,
                    (i / 7) as u8,
                    if i < 100 { 0 } else { 255 },
                ]
            })
            .collect();
        let icon = optimize(&rgba);
        assert_eq!(icon.pixels.len(), 1024);
        assert!(icon.pixels.iter().all(|&i| i < 16));
        assert!(icon.palette.iter().flatten().all(|c| c % 17 == 0));
        assert!(icon.rgba()[..400].chunks_exact(4).all(|p| p[3] == 0));
        let rgba: Vec<u8> = (0..1024)
            .flat_map(|i| match i {
                0 => [0, 0, 0, 255],
                1 => [0, 0, 0, 0],
                _ => [255; 4],
            })
            .collect();
        let mono = monochrome(&rgba, 128);
        assert_eq!(mono[0], 0x80);
        assert!(mono[1..].iter().all(|&b| b == 0));
    }
}
