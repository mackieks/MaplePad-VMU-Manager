use crate::vmu::{self, Firmware, ImageFormat, VmuImage};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use tempfile::TempDir;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const PICOTOOL_ZLIB: &[u8] = include_bytes!("../assets/picotool.exe.zlib");

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeviceId {
    pub bus: Option<u8>,
    pub address: Option<u8>,
}

impl DeviceId {
    fn append_args(&self, args: &mut Vec<String>) {
        if let (Some(bus), Some(address)) = (self.bus, self.address) {
            args.extend([
                "--bus".into(),
                bus.to_string(),
                "--address".into(),
                address.to_string(),
            ]);
        }
    }

    pub fn label(&self) -> String {
        match (self.bus, self.address) {
            (Some(bus), Some(address)) => format!("USB bus {bus}, address {address}"),
            _ => "USB device".into(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct SlotState {
    pub image: Option<VmuImage>,
    pub error: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Device {
    pub id: DeviceId,
    pub firmware: Firmware,
    pub slots: Vec<SlotState>,
}

pub struct Picotool {
    executable: PathBuf,
    workspace: TempDir,
    next_file: AtomicU64,
}

impl Picotool {
    pub fn new() -> Result<Self, String> {
        let workspace = tempfile::Builder::new()
            .prefix("maplepad-vmu-manager-")
            .tempdir()
            .map_err(|error| format!("Cannot create temporary directory: {error}"))?;
        let executable = workspace.path().join("picotool.exe");
        {
            let mut output = fs::File::create(&executable)
                .map_err(|error| format!("Cannot create embedded picotool: {error}"))?;
            let mut decoder = flate2::read::ZlibDecoder::new(PICOTOOL_ZLIB);
            std::io::copy(&mut decoder, &mut output)
                .map_err(|error| format!("Cannot unpack embedded picotool: {error}"))?;
        }
        let tool = Self {
            executable,
            workspace,
            next_file: AtomicU64::new(0),
        };
        let version = tool.run(&["version".into()])?;
        if version.contains("without USB support") {
            return Err("Embedded picotool was built without USB support".into());
        }
        Ok(tool)
    }

    fn temp_path(&self, name: &str) -> PathBuf {
        let number = self.next_file.fetch_add(1, Ordering::Relaxed);
        self.workspace.path().join(format!("{name}-{number}.bin"))
    }

    fn run(&self, args: &[String]) -> Result<String, String> {
        let output = Command::new(&self.executable)
            .args(args)
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|error| format!("Could not start picotool: {error}"))?;
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if output.status.success() {
            Ok(stdout)
        } else {
            Err(if stderr.is_empty() { stdout } else { stderr })
        }
    }

    pub fn scan(&self) -> Result<Vec<Device>, String> {
        let info = match self.run(&["info".into(), "-d".into()]) {
            Ok(info) => info,
            Err(error) if error.contains("No accessible RP-series devices") => {
                return Ok(Vec::new())
            }
            Err(error) => return Err(format!("Device discovery failed: {error}")),
        };
        let ids = device_ids_from_info(&info)?;
        let mut devices = Vec::new();
        for id in ids {
            let settings = match self.read_flash(&id, vmu::SETTINGS_ADDRESS, 256) {
                Ok(bytes) => bytes,
                Err(_) => continue,
            };
            let Some(firmware) = vmu::firmware_from_settings(&settings) else {
                continue; // An RP-series board, but not a recognized MaplePad.
            };
            let all_pages = match self.read_flash(
                &id,
                vmu::FIRST_SLOT_ADDRESS,
                vmu::IMAGE_SIZE * vmu::SLOT_COUNT,
            ) {
                Ok(bytes) => bytes,
                Err(error) => {
                    let slots = (0..vmu::SLOT_COUNT)
                        .map(|_| SlotState {
                            image: None,
                            error: Some(error.clone()),
                        })
                        .collect();
                    devices.push(Device {
                        id,
                        firmware,
                        slots,
                    });
                    continue;
                }
            };
            let slots = all_pages
                .chunks_exact(vmu::IMAGE_SIZE)
                .map(|page| match vmu::parse_image(page) {
                    Ok(image) => SlotState {
                        image: Some(image),
                        error: None,
                    },
                    Err(error) => SlotState {
                        image: None,
                        error: Some(error),
                    },
                })
                .collect();
            devices.push(Device {
                id,
                firmware,
                slots,
            });
        }
        Ok(devices)
    }

    fn read_flash(&self, id: &DeviceId, address: u32, length: usize) -> Result<Vec<u8>, String> {
        let end = address
            .checked_add(length as u32)
            .ok_or("Flash address overflow")?;
        let path = self.temp_path("read");
        let mut args = vec![
            "save".into(),
            "-r".into(),
            format!("{address:08X}"),
            format!("{end:08X}"),
            path.to_string_lossy().into_owned(),
            "-t".into(),
            "bin".into(),
        ];
        id.append_args(&mut args);
        let result = self.run(&args);
        if let Err(error) = result {
            let _ = fs::remove_file(&path);
            return Err(format!("picotool save failed: {error}"));
        }
        let bytes =
            fs::read(&path).map_err(|error| format!("Cannot read picotool output: {error}"));
        let _ = fs::remove_file(&path);
        let bytes = bytes?;
        if bytes.len() != length {
            return Err(format!(
                "picotool returned {} bytes; expected {length}",
                bytes.len()
            ));
        }
        Ok(bytes)
    }

    pub fn dump_slot(
        &self,
        id: &DeviceId,
        slot: usize,
        destination: &Path,
    ) -> Result<VmuImage, String> {
        if destination.exists() {
            return Err(format!("Refusing to overwrite {}", destination.display()));
        }
        let address = vmu::slot_address(slot)?;
        let raw = self.read_flash(id, address, vmu::IMAGE_SIZE)?;
        let image = vmu::parse_image(&raw)?;
        write_new(destination, &image.bytes)?;
        Ok(image)
    }

    pub fn dump_all(&self, id: &DeviceId, folder: &Path) -> Result<Vec<VmuImage>, String> {
        if !folder.is_dir() {
            return Err(format!(
                "Output folder does not exist: {}",
                folder.display()
            ));
        }
        let paths: Vec<_> = (0..vmu::SLOT_COUNT)
            .map(|slot| folder.join(format!("vmu{slot}.bin")))
            .collect();
        for path in &paths {
            if path.exists() {
                return Err(format!("Refusing to overwrite {}", path.display()));
            }
        }
        // Read and validate all pages before writing any output.
        let mut images = Vec::new();
        for slot in 0..vmu::SLOT_COUNT {
            let raw = self.read_flash(id, vmu::slot_address(slot)?, vmu::IMAGE_SIZE)?;
            images.push(vmu::parse_image(&raw)?);
        }
        for (path, image) in paths.iter().zip(&images) {
            write_new(path, &image.bytes)?;
        }
        Ok(images)
    }

    pub fn load_slot(
        &self,
        id: &DeviceId,
        slot: usize,
        input: &Path,
    ) -> Result<(PathBuf, VmuImage), String> {
        let source =
            fs::read(input).map_err(|error| format!("Cannot read {}: {error}", input.display()))?;
        match vmu::image_format(&source)? {
            ImageFormat::MaplePad15 | ImageFormat::Explorer20 => {}
            ImageFormat::Native20 => {
                return Err("Input is a native flash page, not a PC-side dump".into())
            }
        }
        let backup = input.with_file_name(format!("vmu{slot}.before-load.bin"));
        if backup == input {
            return Err("Backup path matches input".into());
        }
        self.write_slot(id, slot, &source, None, &backup)
    }

    pub fn save_changes(
        &self,
        id: &DeviceId,
        slot: usize,
        source: &[u8],
        expected: &[u8],
        backup: &Path,
    ) -> Result<(PathBuf, VmuImage), String> {
        self.write_slot(id, slot, source, Some(expected), backup)
    }

    fn write_slot(
        &self,
        id: &DeviceId,
        slot: usize,
        source: &[u8],
        expected: Option<&[u8]>,
        backup: &Path,
    ) -> Result<(PathBuf, VmuImage), String> {
        let address = vmu::slot_address(slot)?;
        if backup.exists() {
            return Err(format!("Backup already exists: {}", backup.display()));
        }
        let previous = self.read_flash(id, address, vmu::IMAGE_SIZE)?;
        let target_format = vmu::image_format(&previous)?;
        let firmware = match target_format {
            ImageFormat::MaplePad15 => Firmware::MaplePad15,
            ImageFormat::Native20 => Firmware::MaplePad20,
            ImageFormat::Explorer20 => {
                return Err("Target VMU uses PC byte order; no flash was written".into())
            }
        };
        let native = vmu::prepare_for_target(&source, firmware)?;
        let previous_image = vmu::parse_image(&previous)?;
        if expected.is_some_and(|bytes| bytes != previous_image.bytes) {
            return Err("The MaplePad VMU changed since it was read. No flash was written. Save a dump before discarding edits and refreshing.".into());
        }
        write_new(backup, &previous_image.bytes)?;

        let staged = self.temp_path("write");
        fs::write(&staged, &native).map_err(|error| format!("Cannot stage VMU image: {error}"))?;
        let mut args = vec![
            "load".into(),
            "-v".into(),
            staged.to_string_lossy().into_owned(),
            "-t".into(),
            "bin".into(),
            "-o".into(),
            format!("{address:08X}"),
        ];
        id.append_args(&mut args);
        let result = self.run(&args);
        let _ = fs::remove_file(&staged);
        result.map_err(|error| {
            format!(
                "picotool load failed; keep backup {}: {error}",
                backup.display()
            )
        })?;
        let written = self.read_flash(id, address, vmu::IMAGE_SIZE)?;
        if written != native {
            return Err(format!(
                "Read-back differs from input; keep backup {}",
                backup.display()
            ));
        }
        let updated = vmu::parse_image(&written)?;
        Ok((backup.to_path_buf(), updated))
    }
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| format!("Cannot create {}: {error}", path.display()))?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| format!("Cannot finish {}: {error}", path.display()))
}

fn device_ids_from_info(info: &str) -> Result<Vec<DeviceId>, String> {
    if !info.contains("Multiple RP-series devices") {
        return Ok(vec![DeviceId {
            bus: None,
            address: None,
        }]);
    }
    let mut ids = Vec::new();
    for line in info.lines() {
        let Some((_, tail)) = line.split_once(" device at bus ") else {
            continue;
        };
        let Some((bus, address)) = tail.trim_end_matches(':').split_once(", address ") else {
            continue;
        };
        if let (Ok(bus), Ok(address)) = (bus.parse::<u8>(), address.parse::<u8>()) {
            let id = DeviceId {
                bus: Some(bus),
                address: Some(address),
            };
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    if ids.is_empty() {
        return Err("picotool found multiple devices but did not report USB addresses".into());
    }
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_picotool_extracts_and_runs_version() {
        // Only invokes `version`; no device discovery or hardware writes.
        let tool = Picotool::new().unwrap();
        let bytes = fs::read(&tool.executable).unwrap();
        assert_eq!(bytes.len(), 6_239_513);
        assert_eq!(&bytes[..2], b"MZ");
        assert!(tool.run(&["version".into()]).unwrap().contains("2.3.1"));
    }

    #[test]
    fn parses_multiple_device_selectors() {
        let info = "Multiple RP-series devices in BOOTSEL mode found:\n\nRP2040 device at bus 2, address 3:\n---\nRP2350 device at bus 4, address 5:\n";
        assert_eq!(
            device_ids_from_info(info).unwrap(),
            vec![
                DeviceId {
                    bus: Some(2),
                    address: Some(3)
                },
                DeviceId {
                    bus: Some(4),
                    address: Some(5)
                },
            ]
        );
        assert_eq!(
            device_ids_from_info("Program Information").unwrap().len(),
            1
        );
    }
}
