use super::*;
use std::io::Write;

pub enum FileAction {
    Copy(usize),
    Delete(usize),
    Paste(Option<usize>),
    Move(usize, bool),
    Save,
    Discard,
}

impl ManagerApp {
    pub fn is_dirty(&self) -> bool {
        self.current_image()
            .is_some_and(|i| i.original_bytes.is_some())
    }
    pub fn any_dirty(&self) -> bool {
        self.previews
            .iter()
            .any(|(_, i)| i.original_bytes.is_some())
            || self
                .devices
                .iter()
                .flat_map(|d| &d.slots)
                .filter_map(|s| s.image.as_ref())
                .any(|i| i.original_bytes.is_some())
    }
    pub(super) fn replace_current(&mut self, image: VmuImage, ctx: &egui::Context) {
        if self.preview_active {
            if let Some((_, current)) = self.previews.get_mut(self.selected_slot) {
                *current = image;
            }
        } else if let Some(slot) = self
            .devices
            .get_mut(self.selected_device)
            .and_then(|d| d.slots.get_mut(self.selected_slot))
        {
            slot.image = Some(image);
        }
        self.rebuild_icons(ctx);
    }
    pub(super) fn selected_file_key(&self) -> Option<[u8; 12]> {
        let file = self.current_image()?.files.get(self.selected_file?)?;
        Some(file.directory_entry[4..16].try_into().unwrap())
    }
    pub(super) fn refresh_saved_view(&mut self, ctx: &egui::Context, key: Option<[u8; 12]>) {
        // Texture refresh and model replacement occur in one frame. Preserve the
        // view state; never clear the image/source or reopen it through the UI.
        let hex = (
            self.hex_selection,
            self.hex_anchor,
            self.hex_hover_field,
            self.hex_hover_byte,
        );
        self.rebuild_icons(ctx);
        (
            self.hex_selection,
            self.hex_anchor,
            self.hex_hover_field,
            self.hex_hover_byte,
        ) = hex;
        self.selected_file = key.and_then(|key| {
            self.current_image()?
                .files
                .iter()
                .position(|f| f.directory_entry[4..16] == key)
        });
        ctx.request_repaint();
    }
    pub fn close_all_local(&mut self, ctx: &egui::Context) {
        if self
            .previews
            .iter()
            .any(|(_, i)| i.original_bytes.is_some())
        {
            self.error_popup = Some(
                "Save or discard pending edits in your local images before closing them all."
                    .into(),
            );
            return;
        }
        self.previews.clear();
        if self.preview_active {
            self.preview_active = false;
            self.selected_slot = 0;
            self.selected_file = None;
        }
        self.rebuild_icons(ctx);
    }
    pub fn file_action(&mut self, action: FileAction, ctx: &egui::Context) {
        if self.busy {
            return;
        }
        match action {
            FileAction::Copy(index) => {
                if let Some(image) = self.current_image() {
                    match vmu::editing::copy(image, index) {
                        Ok(file) => {
                            self.status = format!("Copied {} · {} blocks", file.name, file.blocks);
                            self.clipboard = Some(file);
                        }
                        Err(e) => self.error_popup = Some(e),
                    }
                }
            }
            FileAction::Save => self.save_changes(ctx),
            FileAction::Discard => {
                if let Some(image) = self.current_image() {
                    if let Some(original) = &image.original_bytes {
                        let mut restored =
                            vmu::parse_image(original).expect("previously parsed VMU");
                        restored.format = image.original_format.unwrap_or(image.format);
                        self.replace_current(restored, ctx);
                        self.selected_file = None;
                        self.status = "Changes discarded".into();
                    }
                }
            }
            FileAction::Delete(_) | FileAction::Paste(_) | FileAction::Move(_, _) => {
                let Some(image) = self.current_image() else {
                    return;
                };
                let (result, selection) = match action {
                    FileAction::Delete(index) => (vmu::editing::delete(image, index), None),
                    FileAction::Paste(after) => {
                        let Some(file) = &self.clipboard else {
                            return;
                        };
                        let placement = after.map_or(
                            vmu::editing::Placement::FirstFit,
                            vmu::editing::Placement::After,
                        );
                        (
                            vmu::editing::paste(image, file, placement),
                            Some(image.files.len()),
                        )
                    }
                    FileAction::Move(index, higher) => {
                        (vmu::editing::move_file(image, index, higher), Some(index))
                    }
                    _ => unreachable!(),
                };
                match result {
                    Ok(updated) => {
                        self.replace_current(updated, ctx);
                        self.selected_file = selection;
                        self.status =
                            "Unsaved changes · choose Save Changes or Discard Changes".into();
                    }
                    Err(e) => self.error_popup = Some(e),
                }
            }
        }
    }

    fn save_changes(&mut self, ctx: &egui::Context) {
        let Some(image) = self
            .current_image()
            .filter(|i| i.original_bytes.is_some())
            .cloned()
        else {
            return;
        };
        let original = image.original_bytes.as_ref().unwrap().clone();
        if self.preview_active {
            let path = self.previews[self.selected_slot].0.clone();
            match save_local(&path, &image) {
                Ok((backup, verified)) => {
                    let key = self.selected_file_key();
                    self.previews[self.selected_slot].1 = verified;
                    self.refresh_saved_view(ctx, key);
                    self.status = format!("Changes saved. Backup: {}", ui::display_path(&backup));
                }
                Err(e) => self.error_popup = Some(e),
            }
        } else {
            let Some(device) = self.current_device_id() else {
                return;
            };
            let Some(tool) = self.picotool.clone() else {
                return;
            };
            let Some(folder) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) else {
                self.error_popup = Some("Cannot locate the MaplePad backup folder.".into());
                return;
            };
            let slot = self.selected_slot;
            let backup = folder
                .join("MaplePad VMU Manager")
                .join("Backups")
                .join(format!("vmu{slot}.{}.before-edit.bin", timestamp()));
            self.busy = true;
            self.status = format!("Saving VMU {slot}: backup, write, verify…");
            let sender = self.sender.clone();
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let result = fs::create_dir_all(backup.parent().unwrap())
                    .map_err(|e| e.to_string())
                    .and_then(|_| {
                        tool.save_changes(&device, slot, &image.bytes, &original, &backup)
                    });
                let _ = sender.send(Event::Save {
                    device,
                    slot,
                    result,
                });
                ctx.request_repaint();
            });
        }
    }

    pub fn edit_error_dialog(&mut self, ctx: &egui::Context) {
        if let Some(message) = self.error_popup.clone() {
            let mut close = false;
            egui::Window::new("VMU Manager")
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .collapsible(false)
                .resizable(false)
                .default_width(360.0)
                .frame(
                    egui::Frame::window(&ctx.style())
                        .fill(ctx.style().visuals.window_fill)
                        .stroke(egui::Stroke::new(1.0_f32, Color32::from_rgb(160, 35, 50)))
                        .rounding(2.0),
                )
                .show(ctx, |ui| {
                    ui.spacing_mut().item_spacing.y = 10.0;
                    ui.add(egui::Label::new(message).wrap());
                    close = ui.button("OK").clicked();
                });
            if close || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.error_popup = None;
            }
        }
    }
}

fn timestamp() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn save_local(path: &std::path::Path, image: &VmuImage) -> Result<(PathBuf, VmuImage), String> {
    let previous = fs::read(path).map_err(|e| e.to_string())?;
    let parsed = vmu::parse_image(&previous)?;
    if Some(&parsed.bytes) != image.original_bytes.as_ref() {
        return Err(
            "This file changed on disk after it was opened. Nothing was overwritten.".into(),
        );
    }
    let backup = path.with_file_name(format!(
        "{}.{}.before-edit.bin",
        path.file_name().unwrap_or_default().to_string_lossy(),
        timestamp()
    ));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&backup)
        .map_err(|e| e.to_string())?;
    file.write_all(&previous)
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())?;
    let bytes = if image.format == vmu::ImageFormat::Native20 {
        vmu::swap_words(&image.bytes)
    } else {
        image.bytes.clone()
    };
    let mut staged = tempfile::NamedTempFile::new_in(path.parent().ok_or("Missing image folder")?)
        .map_err(|e| e.to_string())?;
    staged
        .write_all(&bytes)
        .and_then(|_| staged.as_file().sync_all())
        .map_err(|e| e.to_string())?;
    staged.persist(path).map_err(|e| {
        format!(
            "Could not replace image; backup at {}: {e}",
            ui::display_path(&backup)
        )
    })?;
    let read_back = fs::read(path).map_err(|e| e.to_string())?;
    if read_back != bytes {
        return Err(format!(
            "Read-back did not match. Backup: {}",
            ui::display_path(&backup)
        ));
    }
    Ok((backup, vmu::parse_image(&read_back)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn populated(capacity: u16) -> Vec<u8> {
        let mut raw = vmu::tests::formatted(capacity);
        let mut first = capacity - 1;
        for (n, (name, blocks)) in [("ALPHA", 4u16), ("BETA", 2), ("GAMMA", 3)]
            .iter()
            .enumerate()
        {
            let at = 253 * 512 + n * 32;
            raw[at..at + 32].fill(0);
            raw[at] = 0x33;
            raw[at + 2..at + 4].copy_from_slice(&first.to_le_bytes());
            raw[at + 4..at + 4 + name.len()].copy_from_slice(name.as_bytes());
            raw[at + 24..at + 26].copy_from_slice(&blocks.to_le_bytes());
            for step in 0..*blocks {
                let block = first - step;
                let next = if step + 1 == *blocks {
                    0xfffa
                } else {
                    block - 1
                };
                let fat = 254 * 512 + block as usize * 2;
                raw[fat..fat + 2].copy_from_slice(&next.to_le_bytes());
                raw[block as usize * 512..(block as usize + 1) * 512].fill(n as u8 + 1);
            }
            first -= blocks;
        }
        raw
    }
    fn assert_view_matches(app: &ManagerApp, raw: &[u8]) {
        let saved = vmu::parse_image(raw).unwrap();
        let view = app.current_image().unwrap();
        assert_eq!(view.bytes, saved.bytes);
        assert_eq!(view.free, saved.free);
        assert_eq!(view.files.len(), saved.files.len());
        for (shown, actual) in view.files.iter().zip(&saved.files) {
            assert_eq!(shown.name, actual.name);
            assert_eq!(shown.first_block, actual.first_block);
            assert_eq!(shown.directory_entry, actual.directory_entry);
        }
        assert!(!app.is_dirty());
    }
    #[test]
    fn move_paste_delete_saves_refresh_metadata_without_reopening_the_local_image() {
        for native in [false, true] {
            let ctx = egui::Context::default();
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("edit.bin");
            let raw = populated(if native { 241 } else { 200 });
            fs::write(&path, if native { vmu::swap_words(&raw) } else { raw }).unwrap();
            let mut app = ManagerApp::create_state(None, String::new());
            app.open_paths(vec![path.clone()], &ctx);
            app.sort_column = sorting::Column::FirstBlock;
            app.sort_descending = true;
            app.file_action(FileAction::Move(1, true), &ctx);
            let moved_first = app.current_image().unwrap().files[1].first_block;
            assert_eq!(moved_first, if native { 240 } else { 199 });
            app.hex_selection = Some((0, 8));
            app.hex_scroll_to = Some(256);
            // A stale derived value must be replaced from verified saved bytes.
            app.previews[0].1.files[1].first_block = 0;
            app.file_action(FileAction::Save, &ctx);
            assert!(app.error_popup.is_none(), "{:?}", app.error_popup);
            assert_view_matches(&app, &fs::read(&path).unwrap());
            assert_eq!(
                app.current_image().unwrap().files[1].first_block,
                moved_first
            );
            assert_eq!(app.selected_file, Some(1));
            assert_eq!(app.hex_selection, Some((0, 8)));
            assert_eq!(app.hex_scroll_to, Some(256));
            assert_eq!(app.previews.len(), 1);
            assert!(app.preview_active && app.sort_descending);
            app.file_action(FileAction::Copy(1), &ctx);
            app.file_action(FileAction::Delete(1), &ctx);
            app.file_action(FileAction::Save, &ctx);
            assert_view_matches(&app, &fs::read(&path).unwrap());
            app.file_action(FileAction::Paste(Some(0)), &ctx);
            app.file_action(FileAction::Save, &ctx);
            assert_view_matches(&app, &fs::read(&path).unwrap());
            assert_eq!(
                app.current_image().unwrap().files[app.selected_file.unwrap()].name,
                "BETA"
            );
        }
    }
    #[test]
    fn verified_maplepad_save_refreshes_first_blocks_and_preserves_the_active_view() {
        let ctx = egui::Context::default();
        let mut app = ManagerApp::create_state(None, String::new());
        let id = DeviceId {
            bus: Some(1),
            address: Some(2),
        };
        let original = vmu::parse_image(&populated(241)).unwrap();
        app.devices.push(Device {
            id: id.clone(),
            firmware: vmu::Firmware::MaplePad20,
            slots: vec![device::SlotState {
                image: Some(original.clone()),
                error: None,
            }],
        });
        app.selected_file = Some(1);
        app.hex_selection = Some((0, 8));
        let moved = vmu::editing::move_file(&original, 1, true).unwrap();
        let verified = vmu::parse_image(&vmu::swap_words(&moved.bytes)).unwrap();
        app.sender
            .send(Event::Save {
                device: id,
                slot: 0,
                result: Ok(("backup.bin".into(), verified)),
            })
            .unwrap();
        app.handle_events(&ctx);
        assert_view_matches(&app, &moved.bytes);
        assert_eq!(app.current_image().unwrap().files[1].first_block, 240);
        assert_eq!(app.selected_file, Some(1));
        assert_eq!(app.hex_selection, Some((0, 8)));
        assert!(!app.preview_active);
    }
    #[test]
    fn local_save_preserves_native_order_backs_up_and_rejects_external_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.bin");
        let original = vmu::swap_words(&vmu::tests::formatted(241));
        fs::write(&path, &original).unwrap();
        let mut image = vmu::parse_image(&original).unwrap();
        image.original_bytes = Some(image.bytes.clone());
        image.bytes[100] ^= 1;
        let (backup, verified) = save_local(&path, &image).unwrap();
        assert_eq!(verified.bytes, image.bytes);
        assert!(verified.original_bytes.is_none());
        assert_eq!(fs::read(backup).unwrap(), original);
        assert_eq!(fs::read(&path).unwrap(), vmu::swap_words(&image.bytes));
        assert!(save_local(&path, &image)
            .unwrap_err()
            .contains("changed on disk"));
    }
}
