use encoding_rs::SHIFT_JIS;
pub mod editing;
pub mod root;

pub const BLOCK_SIZE: usize = 512;
pub const IMAGE_SIZE: usize = 0x20000;
pub const SLOT_COUNT: usize = 8;
pub const FLASH_BASE: u32 = 0x1000_0000;
pub const FIRST_SLOT_ADDRESS: u32 = FLASH_BASE + 0x20000;
pub const SETTINGS_ADDRESS: u32 = FLASH_BASE + 0x120000;
const ROOT_OFFSET: usize = 255 * BLOCK_SIZE;
const FAT_OFFSET: usize = 254 * BLOCK_SIZE;
const FAT_FREE: u16 = 0xfffc;
const LEGACY_GEOMETRY: [u8; 12] = [0xff, 0, 0, 0, 0xff, 0, 0xfe, 0, 1, 0, 0xfd, 0];
const NATIVE_GEOMETRY: [u8; 12] = [0, 0, 0, 0xff, 0, 0xfe, 0, 0xff, 0, 0xfd, 0, 1];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImageFormat {
    MaplePad15,
    Explorer20,
    Native20,
}

impl ImageFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::MaplePad15 => "MaplePad 1.5",
            Self::Explorer20 | Self::Native20 => "MaplePad 2.0",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Firmware {
    MaplePad15,
    MaplePad20,
}

impl Firmware {
    pub fn label(self) -> &'static str {
        match self {
            Self::MaplePad15 => "1.5",
            Self::MaplePad20 => "2.0",
        }
    }
}

#[derive(Clone, Debug)]
pub struct HeaderField {
    pub name: String,
    pub offset: usize,
    pub size: usize,
    pub value: String,
}

#[derive(Clone, Debug)]
pub struct VmuFile {
    pub directory_entry: [u8; 32],
    pub name: String,
    pub kind: &'static str,
    pub blocks: u16,
    pub first_block: u16,
    pub copy_protected: bool,
    pub vm_description: String,
    pub dc_description: String,
    pub application: String,
    pub icon_rgba: Option<Vec<u8>>,
    pub icon_frames: Vec<Vec<u8>>,
    pub palette: Option<[[u8; 4]; 16]>,
    pub mono_icon: Option<Vec<u8>>,
    pub created: String,
    pub crc: Option<u16>,
    pub crc_valid: Option<bool>,
    pub icon_count: u16,
    pub animation_speed: u16,
    pub eyecatch_type: u16,
    pub data_size: u32,
    pub header_offset: usize,
    pub fields: Vec<HeaderField>,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct VmuImage {
    pub original_bytes: Option<Vec<u8>>,
    pub original_format: Option<ImageFormat>,
    pub format: ImageFormat,
    pub bytes: Vec<u8>, // Always in VMU Explorer byte order.
    pub capacity: usize,
    pub free: usize,
    pub files: Vec<VmuFile>,
    pub card_icon_rgba: Option<Vec<u8>>,
    pub card_color: Option<[u8; 4]>,
}

pub fn slot_address(slot: usize) -> Result<u32, String> {
    if slot >= SLOT_COUNT {
        return Err(format!("VMU slot {slot} is outside 0..7"));
    }
    Ok(FIRST_SLOT_ADDRESS + slot as u32 * IMAGE_SIZE as u32)
}

pub fn image_format(image: &[u8]) -> Result<ImageFormat, String> {
    if image.len() != IMAGE_SIZE {
        return Err(format!(
            "Expected a 128 KiB VMU image, got {} bytes",
            image.len()
        ));
    }
    let root = &image[ROOT_OFFSET..ROOT_OFFSET + BLOCK_SIZE];
    if root[..16] != [0x55; 16] {
        return Err("VMU root magic is absent (blank or damaged page)".into());
    }
    if root[0x40..0x4c] == LEGACY_GEOMETRY && root[0x4c..0x4e] == [0x0d, 0] {
        return match &root[0x50..0x52] {
            [0xc8, 0] => Ok(ImageFormat::MaplePad15),
            [0xf1, 0] => Ok(ImageFormat::Explorer20),
            _ => Err("Unrecognized VMU save area size".into()),
        };
    }
    if root[0x40..0x4c] == NATIVE_GEOMETRY
        && root[0x4e..0x50] == [0, 0x0d]
        && root[0x52..0x54] == [0, 0xf1]
    {
        return Ok(ImageFormat::Native20);
    }
    Err("VMU root geometry does not match MaplePad 1.5 or 2.0".into())
}

pub fn swap_words(image: &[u8]) -> Vec<u8> {
    image
        .chunks_exact(4)
        .flat_map(|word| word.iter().rev().copied())
        .collect()
}

pub fn firmware_from_settings(settings: &[u8]) -> Option<Firmware> {
    if settings.len() < 34 {
        return None;
    }
    if settings[0] == 0x0d && settings[1] == 0 {
        Some(Firmware::MaplePad20)
    } else if settings[33] == 0x0a && settings[14] == 0 {
        Some(Firmware::MaplePad15)
    } else {
        None
    }
}

pub fn prepare_for_target(source: &[u8], firmware: Firmware) -> Result<Vec<u8>, String> {
    let source_format = image_format(source)?;
    if source_format == ImageFormat::Native20 {
        return Err("Select a VMU Explorer-compatible dump, not a native flash page".into());
    }
    let mut image = source.to_vec();
    match firmware {
        Firmware::MaplePad15 => {
            if source_format == ImageFormat::Explorer20 {
                for block in 200..241 {
                    if fat_entry(&image, block) != FAT_FREE {
                        return Err(format!(
                            "Block {block} is allocated in the 2.0-only area; cannot restore to 1.5"
                        ));
                    }
                }
                for block in 0..200 {
                    let successor = fat_entry(&image, block) as usize;
                    if (200..241).contains(&successor) {
                        return Err(format!(
                            "Block {block} links to 2.0-only block {successor}; cannot restore to 1.5"
                        ));
                    }
                }
            }
            image[ROOT_OFFSET + 0x50..ROOT_OFFSET + 0x52].copy_from_slice(&[0xc8, 0]);
            Ok(image)
        }
        Firmware::MaplePad20 => {
            image[ROOT_OFFSET + 0x50..ROOT_OFFSET + 0x52].copy_from_slice(&[0xf1, 0]);
            Ok(swap_words(&image))
        }
    }
}

pub fn parse_image(raw: &[u8]) -> Result<VmuImage, String> {
    let format = image_format(raw)?;
    let bytes = if format == ImageFormat::Native20 {
        swap_words(raw)
    } else {
        raw.to_vec()
    };
    let capacity = if format == ImageFormat::MaplePad15 {
        200
    } else {
        241
    };
    let free = (0..capacity)
        .filter(|&block| fat_entry(&bytes, block) == FAT_FREE)
        .count();
    let files = parse_directory(&bytes);
    let card_icon_rgba = files
        .iter()
        .find(|file| file.name == "ICONDATA_VMS")
        .and_then(|file| {
            file.icon_frames.first().cloned().or_else(|| {
                file.mono_icon.as_ref().map(|rgba| {
                    rgba.chunks_exact(4)
                        .flat_map(|pixel| root::bios_icon_pixel(pixel[3] != 0))
                        .collect()
                })
            })
        })
        .or_else(|| {
            let root = &bytes[ROOT_OFFSET..];
            Some(root::bios_mono_rgba(&root::bios_mono(u16_at(root, 0x4e))))
        });
    // Root custom-color enable at 0x10, followed by BGRA (KallistiOS vmu_root_t).
    let root = &bytes[ROOT_OFFSET..];
    let card_color = (root[0x10] != 0).then_some([root[0x13], root[0x12], root[0x11], root[0x14]]);
    Ok(VmuImage {
        original_bytes: None,
        original_format: None,
        format,
        bytes,
        capacity,
        free,
        files,
        card_icon_rgba,
        card_color,
    })
}

fn fat_entry(image: &[u8], block: usize) -> u16 {
    let at = FAT_OFFSET + block * 2;
    u16::from_le_bytes([image[at], image[at + 1]])
}

fn parse_directory(image: &[u8]) -> Vec<VmuFile> {
    let root = &image[ROOT_OFFSET..ROOT_OFFSET + BLOCK_SIZE];
    let first = u16::from_le_bytes([root[0x4a], root[0x4b]]) as usize;
    let count = u16::from_le_bytes([root[0x4c], root[0x4d]]) as usize;
    if first >= 256 || count == 0 || count > 16 || count > first + 1 {
        return Vec::new();
    }
    let mut files = Vec::new();
    for block in (first + 1 - count..=first).rev() {
        for offset in (0..BLOCK_SIZE).step_by(32) {
            let entry = &image[block * BLOCK_SIZE + offset..block * BLOCK_SIZE + offset + 32];
            if entry[0] != 0x33 && entry[0] != 0xcc {
                continue;
            }
            let name = String::from_utf8_lossy(&entry[4..16])
                .trim_end_matches([' ', '\0'])
                .to_string();
            let first_block = u16::from_le_bytes([entry[2], entry[3]]);
            let blocks = u16::from_le_bytes([entry[24], entry[25]]);
            let header_offset = u16::from_le_bytes([entry[26], entry[27]]) as usize;
            let data =
                collect_file(image, first_block as usize, blocks as usize).unwrap_or_default();
            files.push(parse_file(
                entry,
                name,
                first_block,
                blocks,
                header_offset * BLOCK_SIZE,
                data,
            ));
        }
    }
    files
}

fn collect_file(image: &[u8], first: usize, blocks: usize) -> Option<Vec<u8>> {
    if blocks == 0 || blocks > 241 || first >= 241 {
        return None;
    }
    let mut out = Vec::with_capacity(blocks * BLOCK_SIZE);
    let mut visited = [false; 241];
    let mut block = first;
    for index in 0..blocks {
        if block >= 241 || visited[block] {
            return None;
        }
        visited[block] = true;
        out.extend_from_slice(&image[block * BLOCK_SIZE..(block + 1) * BLOCK_SIZE]);
        if index + 1 < blocks {
            block = fat_entry(image, block) as usize;
        }
    }
    Some(out)
}

fn u16_at(data: &[u8], at: usize) -> u16 {
    data.get(at..at + 2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .unwrap_or(0)
}

fn u32_at(data: &[u8], at: usize) -> u32 {
    data.get(at..at + 4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .unwrap_or(0)
}

fn parse_file(
    entry: &[u8],
    name: String,
    first_block: u16,
    blocks: u16,
    header_offset: usize,
    data: Vec<u8>,
) -> VmuFile {
    let is_icon = name == "ICONDATA_VMS";
    let header = data.get(header_offset..).unwrap_or_default();
    let mut file = VmuFile {
        directory_entry: entry.try_into().expect("32-byte directory entry"),
        name,
        kind: if is_icon {
            "ICON"
        } else if entry[0] == 0xcc {
            "GAME"
        } else {
            "DATA"
        },
        first_block,
        blocks,
        copy_protected: entry[1] != 0,
        vm_description: String::new(),
        dc_description: String::new(),
        application: String::new(),
        icon_rgba: None,
        icon_frames: Vec::new(),
        palette: None,
        mono_icon: None,
        created: decode_date(&entry[16..24]),
        crc: None,
        crc_valid: None,
        icon_count: 0,
        animation_speed: 0,
        eyecatch_type: 0,
        data_size: 0,
        header_offset,
        fields: Vec::new(),
        bytes: Vec::new(),
    };
    let mut field = |name: &str, offset: usize, size: usize, value: String| {
        file.fields.push(HeaderField {
            name: name.into(),
            offset,
            size,
            value,
        });
    };
    if is_icon && data.len() >= 24 {
        file.vm_description = decode_text(&data[..16]);
        let mono = u32_at(&data, 0x10) as usize;
        let color = u32_at(&data, 0x14) as usize;
        field("Comment", 0, 16, file.vm_description.clone());
        field("Mono Icon Offset", 0x10, 4, format!("0x{mono:08X}"));
        field("Color Icon Offset", 0x14, 4, format!("0x{color:08X}"));
        if let Some(bitmap) = mono
            .checked_add(128)
            .and_then(|end| data.get(mono..end))
            .filter(|_| mono >= 24)
        {
            let mut rgba = Vec::with_capacity(4096);
            for byte in bitmap {
                for bit in (0..8).rev() {
                    rgba.extend_from_slice(if byte & (1 << bit) != 0 {
                        &[0, 0, 0, 255]
                    } else {
                        &[0, 0, 0, 0]
                    });
                }
            }
            file.mono_icon = Some(rgba);
            field("Mono Icon (32 × 32 1bpp)", mono, 128, "128 bytes".into());
        }
        if let Some(blob) = color
            .checked_add(544)
            .and_then(|end| data.get(color..end))
            .filter(|_| color >= 24)
        {
            file.palette = Some(decode_palette(&blob[..32]));
            if let Some(rgba) = decode_color_icon(&blob[..32], &blob[32..]) {
                file.icon_frames.push(rgba);
            }
            field("Color Palette (16 colors)", color, 32, "32 bytes".into());
            field(
                "Color Icon (32 × 32 4bpp)",
                color + 32,
                512,
                "512 bytes".into(),
            );
        }
        file.icon_count = file.icon_frames.len() as u16;
    } else if header.len() >= 128 {
        file.vm_description = decode_text(&header[..16]);
        file.dc_description = decode_text(&header[16..48]);
        file.application = header[48..64]
            .chunks(8)
            .map(|row| {
                row.iter()
                    .map(|b| format!("0x{b:02X}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect::<Vec<_>>()
            .join("\n");
        file.icon_count = u16_at(header, 0x40);
        file.animation_speed = u16_at(header, 0x42);
        file.eyecatch_type = u16_at(header, 0x44);
        file.crc = Some(u16_at(header, 0x46));
        file.data_size = u32_at(header, 0x48);
        file.palette = Some(decode_palette(&header[0x60..0x80]));
        for index in 0..usize::from(file.icon_count).min(256) {
            let offset = 128 + index * 512;
            let Some(bitmap) = header.get(offset..offset + 512) else {
                break;
            };
            if let Some(rgba) = decode_color_icon(&header[0x60..0x80], bitmap) {
                file.icon_frames.push(rgba);
            }
        }
        for (name, at, size, value) in [
            ("VM Description", 0, 16, file.vm_description.clone()),
            ("DC Description", 0x10, 32, file.dc_description.clone()),
            ("Application", 0x30, 16, file.application.replace('\n', " ")),
            ("Icon Count", 0x40, 2, file.icon_count.to_string()),
            ("Anim Speed", 0x42, 2, file.animation_speed.to_string()),
            (
                "Eyecatch Type",
                0x44,
                2,
                format!(
                    "{} · {}",
                    file.eyecatch_type,
                    eyecatch_label(file.eyecatch_type)
                ),
            ),
            ("CRC", 0x46, 2, format!("0x{:04X}", file.crc.unwrap())),
            ("Data Size", 0x48, 4, file.data_size.to_string()),
            ("Reserved", 0x4c, 20, String::new()),
            ("Icon Palette (16 colors)", 0x60, 32, "32 bytes".into()),
            (
                "Icon Bitmaps",
                0x80,
                usize::from(file.icon_count) * 512,
                format!("{} icon(s)", file.icon_count),
            ),
        ] {
            field(name, header_offset + at, size, value);
        }
        let eye_size = match file.eyecatch_type {
            1 => 8064,
            2 => 4544,
            3 => 2048,
            _ => 0,
        };
        let eye_offset = header_offset + 128 + usize::from(file.icon_count) * 512;
        if eye_size > 0 {
            field(
                "Eyecatch",
                eye_offset,
                eye_size,
                eyecatch_label(file.eyecatch_type).into(),
            );
        }
        let payload = eye_offset + eye_size;
        field(
            "File Data",
            payload,
            file.data_size as usize,
            format!("{} bytes", file.data_size),
        );
        if file.kind == "DATA" {
            if let Some(end) = payload
                .checked_add(file.data_size as usize)
                .filter(|end| *end <= data.len())
            {
                file.crc_valid =
                    Some(crc16(&data[..end], header_offset + 0x46) == file.crc.unwrap());
            }
        }
    }
    file.icon_rgba = file
        .icon_frames
        .first()
        .cloned()
        .or_else(|| file.mono_icon.clone());
    file.bytes = data;
    file
}

pub fn eyecatch_label(kind: u16) -> &'static str {
    match kind {
        0 => "None",
        1 => "16-bit color",
        2 => "256 colors",
        3 => "16 colors",
        _ => "Unknown",
    }
}

fn decode_date(bytes: &[u8]) -> String {
    let bcd = |b: u8| (b >> 4) as u16 * 10 + (b & 15) as u16;
    if bytes.len() < 7 || bytes[..7].iter().any(|b| b >> 4 > 9 || b & 15 > 9) {
        return "—".into();
    }
    format!(
        "{:04}/{:02}/{:02} {:02}:{:02}:{:02}",
        bcd(bytes[0]) * 100 + bcd(bytes[1]),
        bcd(bytes[2]),
        bcd(bytes[3]),
        bcd(bytes[4]),
        bcd(bytes[5]),
        bcd(bytes[6])
    )
}

fn crc16(data: &[u8], crc_offset: usize) -> u16 {
    let mut crc = 0u16;
    for (index, byte) in data.iter().enumerate() {
        let byte = if (crc_offset..crc_offset + 2).contains(&index) {
            0
        } else {
            *byte
        };
        crc ^= (byte as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
        }
    }
    crc
}

fn decode_palette(bytes: &[u8]) -> [[u8; 4]; 16] {
    let mut colors = [[0; 4]; 16];
    for (index, color) in colors.iter_mut().enumerate() {
        let value = u16_at(bytes, index * 2);
        *color = [
            ((value >> 8) as u8 & 15) * 17,
            ((value >> 4) as u8 & 15) * 17,
            (value as u8 & 15) * 17,
            ((value >> 12) as u8 & 15) * 17,
        ];
    }
    colors
}

fn decode_text(bytes: &[u8]) -> String {
    let bytes = bytes.trim_ascii_end();
    let end = bytes
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(bytes.len());
    let (text, _, _) = SHIFT_JIS.decode(&bytes[..end]);
    text.trim().to_string()
}

fn decode_color_icon(palette: &[u8], pixels: &[u8]) -> Option<Vec<u8>> {
    if palette.len() != 32 || pixels.len() != 512 {
        return None;
    }
    let mut colors = [[0u8; 4]; 16];
    for (index, color) in colors.iter_mut().enumerate() {
        let value = u16::from_le_bytes([palette[index * 2], palette[index * 2 + 1]]);
        *color = [
            ((value >> 8) as u8 & 0x0f) * 17,
            ((value >> 4) as u8 & 0x0f) * 17,
            (value as u8 & 0x0f) * 17,
            ((value >> 12) as u8 & 0x0f) * 17,
        ];
    }
    let mut rgba = Vec::with_capacity(32 * 32 * 4);
    for &byte in pixels {
        rgba.extend_from_slice(&colors[(byte >> 4) as usize]);
        rgba.extend_from_slice(&colors[(byte & 0x0f) as usize]);
    }
    Some(rgba)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn formatted(capacity: u16) -> Vec<u8> {
        let mut image = vec![0xff; IMAGE_SIZE];
        image[ROOT_OFFSET..ROOT_OFFSET + 16].fill(0x55);
        image[ROOT_OFFSET + 0x40..ROOT_OFFSET + 0x4c].copy_from_slice(&LEGACY_GEOMETRY);
        image[ROOT_OFFSET + 0x4c..ROOT_OFFSET + 0x4e].copy_from_slice(&[0x0d, 0]);
        image[ROOT_OFFSET + 0x50..ROOT_OFFSET + 0x52].copy_from_slice(&capacity.to_le_bytes());
        image[FAT_OFFSET..FAT_OFFSET + BLOCK_SIZE].fill(0xfc);
        for block in 0..256 {
            image[FAT_OFFSET + block * 2 + 1] = 0xff;
        }
        image
    }

    #[test]
    fn root_colors_follow_enable_bgra_alpha_and_native_byte_order() {
        for capacity in [200, 241] {
            let mut image = formatted(capacity);
            image[ROOT_OFFSET + 0x10..ROOT_OFFSET + 0x15]
                .copy_from_slice(&[1, 0x12, 0x34, 0xAB, 0x96]);
            assert_eq!(
                parse_image(&image).unwrap().card_color,
                Some([0xAB, 0x34, 0x12, 0x96])
            );
            if capacity == 241 {
                assert_eq!(
                    parse_image(&swap_words(&image)).unwrap().card_color,
                    Some([0xAB, 0x34, 0x12, 0x96])
                );
            }
            image[ROOT_OFFSET + 0x14] = 0;
            assert_eq!(parse_image(&image).unwrap().card_color.unwrap()[3], 0);
            image[ROOT_OFFSET + 0x10] = 0;
            assert_eq!(parse_image(&image).unwrap().card_color, None);
        }
    }

    #[test]
    fn formats_and_byte_order_conversion_roundtrip() {
        let old = formatted(200);
        let new = formatted(241);
        assert_eq!(image_format(&old).unwrap(), ImageFormat::MaplePad15);
        assert_eq!(image_format(&new).unwrap(), ImageFormat::Explorer20);
        assert_eq!(
            image_format(&swap_words(&new)).unwrap(),
            ImageFormat::Native20
        );
        assert_eq!(
            prepare_for_target(&old, Firmware::MaplePad20).unwrap(),
            swap_words(&new)
        );
        assert_eq!(prepare_for_target(&new, Firmware::MaplePad15).unwrap(), old);
    }

    #[test]
    fn rejects_unsafe_downgrade() {
        let mut image = formatted(241);
        image[FAT_OFFSET + 400..FAT_OFFSET + 402].copy_from_slice(&0xfffau16.to_le_bytes());
        assert!(prepare_for_target(&image, Firmware::MaplePad15).is_err());
    }

    #[test]
    fn firmware_offsets_are_distinct() {
        let mut settings = [0xff; 256];
        settings[0] = 0x0d;
        settings[1] = 0;
        assert_eq!(
            firmware_from_settings(&settings),
            Some(Firmware::MaplePad20)
        );
        settings[0] = 0xff;
        settings[33] = 0x0a;
        settings[14] = 0;
        assert_eq!(
            firmware_from_settings(&settings),
            Some(Firmware::MaplePad15)
        );
    }

    #[test]
    fn directory_and_icondata_are_read_only() {
        let mut image = formatted(241);
        let entry = 253 * BLOCK_SIZE;
        image[entry] = 0x33;
        image[entry + 2..entry + 4].copy_from_slice(&198u16.to_le_bytes());
        image[entry + 4..entry + 16].copy_from_slice(b"ICONDATA_VMS");
        image[entry + 24..entry + 26].copy_from_slice(&1u16.to_le_bytes());
        let parsed = parse_image(&image).unwrap();
        assert_eq!(parsed.files.len(), 1);
        assert_eq!(parsed.files[0].name, "ICONDATA_VMS");
        assert_eq!(parsed.free, 241);
    }

    #[test]
    fn reads_all_animation_frames_and_header_ranges() {
        let mut entry = [0u8; 32];
        entry[0] = 0x33;
        entry[16..23].copy_from_slice(&[0x19, 0x98, 0x11, 0x27, 0x00, 0x27, 0x57]);
        let mut data = vec![0; 128 + 3 * 512 + 4];
        data[..14].copy_from_slice(b"MAIN SAVE FILE");
        data[0x30..0x34].copy_from_slice(&[0x41, 0x00, 0xFF, 0x82]);
        data[0x40..0x42].copy_from_slice(&3u16.to_le_bytes());
        data[0x42..0x44].copy_from_slice(&15u16.to_le_bytes());
        data[0x48..0x4c].copy_from_slice(&4u32.to_le_bytes());
        data[0x60..0x62].copy_from_slice(&0xff00u16.to_le_bytes());
        data[0x62..0x64].copy_from_slice(&0xf00fu16.to_le_bytes());
        data[128 + 512..128 + 1024].fill(0x11);
        let crc = crc16(&data, 0x46);
        data[0x46..0x48].copy_from_slice(&crc.to_le_bytes());
        let file = parse_file(&entry, "TEST".into(), 1, 4, 0, data);
        assert_eq!(file.icon_frames.len(), 3);
        assert_eq!(file.animation_speed, 15);
        assert_eq!(
            file.application,
            "0x41 0x00 0xFF 0x82 0x00 0x00 0x00 0x00\n0x00 0x00 0x00 0x00 0x00 0x00 0x00 0x00"
        );
        assert_eq!(&file.icon_frames[0][..4], &[255, 0, 0, 255]);
        assert_eq!(&file.icon_frames[1][..4], &[0, 0, 255, 255]);
        assert_eq!(file.created, "1998/11/27 00:27:57");
        assert_eq!(file.crc_valid, Some(true));
        let icons = file
            .fields
            .iter()
            .find(|f| f.name == "Icon Bitmaps")
            .unwrap();
        assert_eq!((icons.offset, icons.size), (128, 1536));
    }

    #[test]
    fn reads_mono_icon_and_handles_invalid_offsets() {
        let entry = [0u8; 32];
        let mut data = vec![0; 160];
        data[0x10..0x14].copy_from_slice(&32u32.to_le_bytes());
        data[0x14..0x18].copy_from_slice(&u32::MAX.to_le_bytes());
        data[32] = 0x80;
        let file = parse_file(&entry, "ICONDATA_VMS".into(), 0, 1, 0, data);
        assert_eq!(file.kind, "ICON");
        assert!(file.icon_frames.is_empty());
        let mono = file.mono_icon.unwrap();
        assert_eq!(&mono[..8], &[0, 0, 0, 255, 0, 0, 0, 0]);
    }
}

// Sega VMU Tutorial VMT-18: each stored unit is 1/30 s (two 60 Hz ticks).
pub fn animation_seconds(speed: u16) -> f64 {
    f64::from(speed.max(1)) / 30.0
}

pub fn animation_frame(count: usize, speed: u16, elapsed: f64) -> usize {
    if count == 0 {
        0
    } else {
        ((elapsed / animation_seconds(speed)).floor() as usize) % count
    }
}

#[cfg(test)]
mod timing_tests {
    use super::*;
    #[test]
    fn animation_uses_elapsed_time_and_header_duration() {
        assert_eq!(animation_seconds(15), 0.5);
        for fps in [30, 60, 144] {
            assert_eq!(animation_frame(3, 15, f64::from(fps) / f64::from(fps)), 2);
        }
        assert_eq!(animation_frame(3, 15, 0.499), 0);
        assert_eq!(animation_frame(3, 15, 0.5), 1);
        assert_eq!(animation_frame(3, 15, 1.5), 0);
        assert_eq!(animation_frame(0, 0, 10.0), 0);
    }
}
