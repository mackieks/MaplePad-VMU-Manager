#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod bios_preview;
mod device;
mod edits;
mod export;
mod music;
mod native_animation;
mod navigation;
mod root_editor;
mod scenery;
mod sorting;
mod theme;
mod titlebar;
mod ui;
mod vmu;

use device::{Device, DeviceId, Picotool};
use eframe::egui::{self, Color32, RichText, TextureHandle};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{mpsc, Arc};
use vmu::VmuImage;

fn main() -> eframe::Result {
    let viewport = egui::ViewportBuilder::default()
        .with_inner_size([1200.0, 860.0])
        .with_min_inner_size([760.0, 590.0])
        .with_decorations(false)
        .with_icon(Arc::new(app_icon()));
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "MaplePad VMU Manager",
        options,
        Box::new(|cc| Ok(Box::new(ManagerApp::new(cc)))),
    )
}

fn app_icon() -> egui::IconData {
    let image = image::load_from_memory(include_bytes!("../assets/maple-icon.png"))
        .expect("embedded maple icon")
        .to_rgba8();
    egui::IconData {
        width: image.width(),
        height: image.height(),
        rgba: image.into_raw(),
    }
}

enum Event {
    Created(Result<Option<PathBuf>, String>),
    CopySaved(Result<Option<PathBuf>, String>),
    RootImported {
        mono: bool,
        result: Result<Option<Vec<u8>>, String>,
    },
    OpenImages(Option<Vec<PathBuf>>),
    LoadChosen(Result<Option<PendingLoad>, String>),
    DialogDone(Result<Option<String>, String>),
    Save {
        device: DeviceId,
        slot: usize,
        result: Result<(PathBuf, VmuImage), String>,
    },
    Scan(Result<Vec<Device>, String>),
    Dump {
        device: DeviceId,
        slot: usize,
        path: PathBuf,
        result: Result<VmuImage, String>,
    },
    DumpAll {
        device: DeviceId,
        folder: PathBuf,
        result: Result<Vec<VmuImage>, String>,
    },
    Load {
        device: DeviceId,
        slot: usize,
        result: Result<(PathBuf, VmuImage), String>,
    },
}

struct PendingLoad {
    device: DeviceId,
    slot: usize,
    input: PathBuf,
    description: String,
    maplepad: String,
}

#[derive(Clone, Copy, PartialEq)]
enum DetailTab {
    Detail,
    Hex,
}

struct ManagerApp {
    music: Option<music::Music>,
    root_editor: Option<root_editor::Editor>,
    clipboard: Option<vmu::VmuFile>,
    error_popup: Option<String>,
    scenery: Option<scenery::Scenery>,
    titlebar: Option<titlebar::TitleBar>,
    picotool: Option<Arc<Picotool>>,
    devices: Vec<Device>,
    previews: Vec<(PathBuf, VmuImage)>,
    preview_active: bool,
    selected_device: usize,
    selected_slot: usize,
    selected_file: Option<usize>,
    navigation_scroll: bool,
    card_scroll: bool,
    icons: HashMap<(usize, usize, usize), ui::IconTextures>,
    card_icons: HashMap<(usize, usize), TextureHandle>,
    status: String,
    busy: bool,
    dialog_open: bool,
    file_dialog: rfd::FileDialog,
    pending_load: Option<PendingLoad>,
    detail_tab: DetailTab,
    view_mode: ui::ViewMode,
    top_height: f32,
    middle_height: f32,
    detail_width: f32,
    hex_width: f32,
    hex_selection: Option<(usize, usize)>,
    hex_anchor: Option<usize>,
    hex_scroll_to: Option<usize>,
    hex_hover_field: Option<(usize, usize)>,
    hex_hover_byte: Option<usize>,
    animation_started: std::time::Instant,
    light_theme: bool,
    native_theme: Option<bool>,
    sort_column: sorting::Column,
    sort_descending: bool,
    sender: mpsc::Sender<Event>,
    receiver: mpsc::Receiver<Event>,
}

impl ManagerApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        native_animation::install(cc);
        ui::configure(&cc.egui_ctx);
        let light_theme = theme::load_light();
        theme::apply(&cc.egui_ctx, light_theme);

        let (picotool, status) = match Picotool::new() {
            Ok(tool) => (Some(Arc::new(tool)), "Embedded picotool ready".into()),
            Err(error) => (None, error),
        };
        let mut app = Self::create_state(picotool, status);
        app.file_dialog = rfd::FileDialog::new().set_parent(cc);
        app.music = Some(music::Music::new(music::load_enabled()));
        app.light_theme = light_theme;
        if app.picotool.is_some() {
            app.start_scan(&cc.egui_ctx);
        }
        app.open_paths(
            std::env::args_os().skip(1).map(PathBuf::from).collect(),
            &cc.egui_ctx,
        );
        app
    }

    fn create_state(picotool: Option<Arc<Picotool>>, status: String) -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            music: None,
            root_editor: None,
            clipboard: None,
            error_popup: None,
            scenery: None,
            titlebar: None,
            picotool,
            devices: Vec::new(),
            previews: Vec::new(),
            preview_active: false,
            selected_device: 0,
            selected_slot: 0,
            selected_file: None,
            navigation_scroll: false,
            card_scroll: false,
            icons: HashMap::new(),
            card_icons: HashMap::new(),
            status,
            busy: false,
            dialog_open: false,
            file_dialog: rfd::FileDialog::new(),
            pending_load: None,
            detail_tab: DetailTab::Detail,
            view_mode: ui::ViewMode::List,
            top_height: 140.0,
            middle_height: 280.0,
            detail_width: 425.0,
            hex_width: 590.0,
            hex_selection: None,
            hex_anchor: None,
            hex_scroll_to: None,
            hex_hover_field: None,
            hex_hover_byte: None,
            animation_started: std::time::Instant::now(),
            light_theme: false,
            native_theme: None,
            sort_column: sorting::Column::Name,
            sort_descending: false,
            sender,
            receiver,
        }
    }

    fn current_device(&self) -> Option<&Device> {
        if self.preview_active {
            None
        } else {
            self.devices.get(self.selected_device)
        }
    }
    fn current_image(&self) -> Option<&VmuImage> {
        if self.preview_active {
            self.previews
                .get(self.selected_slot)
                .map(|(_, image)| image)
        } else {
            self.current_device()?
                .slots
                .get(self.selected_slot)?
                .image
                .as_ref()
        }
    }
    fn current_device_id(&self) -> Option<DeviceId> {
        self.current_device().map(|device| device.id.clone())
    }
    fn icon_device_index(&self) -> usize {
        if self.preview_active {
            usize::MAX
        } else {
            self.selected_device
        }
    }

    // Shell pickers must never run inside the egui update/event loop. Keeping
    // that thread pumping avoids Windows ghost captions and frozen artwork.
    fn run_dialog(
        &mut self,
        ctx: &egui::Context,
        work: impl FnOnce(rfd::FileDialog) -> Event + Send + 'static,
    ) {
        if self.busy || self.pending_load.is_some() || self.error_popup.is_some() {
            return;
        }
        self.busy = true;
        self.dialog_open = true;
        let dialog = self.file_dialog.clone();
        let sender = self.sender.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = sender.send(work(dialog));
            ctx.request_repaint();
        });
    }

    fn open_image(&mut self, ctx: &egui::Context) {
        self.run_dialog(ctx, |dialog| {
            Event::OpenImages(
                dialog
                    .add_filter("VMU image", &["bin", "vmu", "vmd"])
                    .pick_files(),
            )
        });
    }

    fn open_paths(&mut self, paths: Vec<PathBuf>, ctx: &egui::Context) {
        let mut errors = Vec::new();
        let mut opened = 0;
        for path in paths {
            let path = fs::canonicalize(&path).unwrap_or(path);
            let result = fs::read(&path)
                .map_err(|e| e.to_string())
                .and_then(|bytes| vmu::parse_image(&bytes));
            match result {
                Ok(image) => {
                    let index = match self.previews.iter().position(|(p, _)| *p == path) {
                        Some(index) => {
                            if self.previews[index].1.original_bytes.is_none() {
                                self.previews[index].1 = image;
                            }
                            index
                        }
                        None => {
                            self.previews.push((path, image));
                            self.previews.len() - 1
                        }
                    };
                    self.preview_active = true;
                    self.selected_slot = index;
                    self.selected_file = None;
                    opened += 1;
                }
                Err(error) => errors.push(format!("{}: {error}", path.display())),
            }
        }
        if opened > 0 {
            self.rebuild_icons(ctx);
            self.status = format!("{} local VMU images open", self.previews.len());
        }
        if !errors.is_empty() {
            self.status = errors.join("; ");
        }
    }

    fn close_local(&mut self, index: usize, ctx: &egui::Context) {
        if index >= self.previews.len() {
            return;
        }
        if self.previews[index].1.original_bytes.is_some() {
            self.error_popup = Some("This image has pending edits. Choose Save Changes or Discard Changes before closing it.".into());
            return;
        }
        self.previews.remove(index);
        if self.preview_active {
            self.selected_slot = self
                .selected_slot
                .saturating_sub(usize::from(index < self.selected_slot))
                .min(self.previews.len().saturating_sub(1));
            if self.previews.is_empty() {
                self.preview_active = false;
                self.selected_slot = 0;
            }
            self.selected_file = None;
        }
        self.rebuild_icons(ctx);
    }

    fn start_scan(&mut self, ctx: &egui::Context) {
        if self
            .devices
            .iter()
            .flat_map(|d| &d.slots)
            .filter_map(|s| s.image.as_ref())
            .any(|i| i.original_bytes.is_some())
        {
            self.error_popup =
                Some("Save or discard pending MaplePad edits before refreshing devices.".into());
            return;
        }
        let Some(tool) = self.picotool.clone() else {
            return;
        };
        if self.busy {
            return;
        }
        self.busy = true;
        self.status = "Scanning BOOTSEL devices and VMU pages…".into();
        let sender = self.sender.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = sender.send(Event::Scan(tool.scan()));
            ctx.request_repaint();
        });
    }

    fn start_dump(&mut self, ctx: &egui::Context) {
        if self.busy || self.current_image().is_none() {
            return;
        }
        let Some(device) = self.current_device_id() else {
            return;
        };
        let Some(tool) = self.picotool.clone() else {
            return;
        };
        let slot = self.selected_slot;
        self.run_dialog(ctx, move |dialog| {
            let Some(path) = dialog
                .add_filter("VMU image", &["bin"])
                .set_file_name(format!("vmu{slot}.bin"))
                .save_file()
            else {
                return Event::DialogDone(Ok(None));
            };
            if path.exists() {
                return Event::DialogDone(Err(format!(
                    "Refusing to overwrite {}",
                    ui::display_path(&path)
                )));
            }
            let result = tool.dump_slot(&device, slot, &path);
            Event::Dump {
                device,
                slot,
                path,
                result,
            }
        });
    }

    fn start_dump_all(&mut self, ctx: &egui::Context) {
        if self.busy || self.current_device().is_none() {
            return;
        }
        let Some(device) = self.current_device_id() else {
            return;
        };
        let Some(tool) = self.picotool.clone() else {
            return;
        };
        self.run_dialog(ctx, move |dialog| {
            let Some(folder) = dialog.pick_folder() else {
                return Event::DialogDone(Ok(None));
            };
            let result = tool.dump_all(&device, &folder);
            Event::DumpAll {
                device,
                folder,
                result,
            }
        });
    }

    fn choose_load(&mut self, ctx: &egui::Context) {
        if self.is_dirty() {
            self.error_popup = Some(
                "Save or discard this VMU's pending edits before loading another image.".into(),
            );
            return;
        }
        if self.busy || self.current_image().is_none() {
            return;
        }
        let Some(device) = self.current_device_id() else {
            return;
        };
        let slot = self.selected_slot;
        let maplepad = self
            .current_device()
            .map(|d| format!("MaplePad {}", d.firmware.label()))
            .unwrap_or_else(|| "MaplePad".into());
        self.run_dialog(ctx, move |dialog| {
            let Some(input) = dialog
                .add_filter("VMU image", &["bin", "vmu", "vmd"])
                .pick_file()
            else {
                return Event::LoadChosen(Ok(None));
            };
            let source_format = match fs::read(&input)
                .and_then(|bytes| vmu::image_format(&bytes).map_err(std::io::Error::other))
            {
                Ok(format) => format,
                Err(error) => {
                    return Event::LoadChosen(Err(format!(
                        "Cannot load {}: {error}",
                        ui::display_path(&input)
                    )))
                }
            };
            if source_format == vmu::ImageFormat::Native20 {
                return Event::LoadChosen(Err(
                    "Select a VMU Explorer-compatible dump, not a native flash page".into(),
                ));
            }
            Event::LoadChosen(Ok(Some(PendingLoad {
                device,
                slot,
                input,
                description: source_format.label().into(),
                maplepad,
            })))
        });
    }

    fn confirm_load(&mut self, ctx: &egui::Context) {
        let Some(pending) = self.pending_load.take() else {
            return;
        };
        let Some(tool) = self.picotool.clone() else {
            return;
        };
        self.busy = true;
        self.status = format!("Backing up and restoring VMU {}…", pending.slot);
        let sender = self.sender.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = tool.load_slot(&pending.device, pending.slot, &pending.input);
            let _ = sender.send(Event::Load {
                device: pending.device,
                slot: pending.slot,
                result,
            });
            ctx.request_repaint();
        });
    }

    fn handle_events(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.receiver.try_recv() {
            self.busy = false;
            self.dialog_open = false;
            match event {
                Event::Created(result) => match result {
                    Ok(Some(path)) => {
                        self.refresh_replaced_local(&path, ctx);
                        self.open_paths(vec![path], ctx);
                        self.open_root_editor();
                    }
                    Ok(None) => {}
                    Err(error) => self.error_popup = Some(error),
                },
                Event::CopySaved(result) => match result {
                    Ok(Some(path)) => {
                        self.refresh_replaced_local(&path, ctx);
                        self.status = format!("Copy saved: {}", ui::display_path(&path));
                    }
                    Ok(None) => {}
                    Err(error) => self.error_popup = Some(error),
                },
                Event::RootImported { mono, result } => {
                    if let Some(editor) = &mut self.root_editor {
                        match result {
                            Ok(Some(rgba)) => editor.import(rgba, mono),
                            Ok(None) => {}
                            Err(error) => editor.error = Some(error),
                        }
                    }
                }
                Event::OpenImages(paths) => {
                    if let Some(paths) = paths {
                        self.open_paths(paths, ctx);
                    }
                }
                Event::LoadChosen(result) => match result {
                    Ok(pending) => self.pending_load = pending,
                    Err(error) => self.error_popup = Some(error),
                },
                Event::DialogDone(result) => match result {
                    Ok(Some(message)) => self.status = message,
                    Ok(None) => {}
                    Err(error) => self.error_popup = Some(error),
                },
                Event::Save {
                    device,
                    slot,
                    result,
                } => match result {
                    Ok((backup, image)) => {
                        let key = self.selected_file_key();
                        if let Some(state) = self
                            .devices
                            .iter_mut()
                            .find(|d| d.id == device)
                            .and_then(|d| d.slots.get_mut(slot))
                        {
                            state.image = Some(image);
                            state.error = None;
                        }
                        self.refresh_saved_view(ctx, key);
                        self.status = format!(
                            "Changes saved and verified. Backup: {}",
                            ui::display_path(&backup)
                        );
                    }
                    Err(error) => self.error_popup = Some(error),
                },
                Event::Scan(result) => match result {
                    Ok(devices) => {
                        self.devices = devices;
                        self.selected_device = 0;
                        if !self.preview_active {
                            self.selected_slot = 0;
                            self.selected_file = None;
                        }
                        self.rebuild_icons(ctx);
                        self.status = match self.devices.len() {
                            0 => "No MaplePad connected".into(),
                            1 => "Ready".into(),
                            n => format!("{n} MaplePads connected"),
                        };
                    }
                    Err(error) => self.status = error,
                },
                Event::Dump {
                    device,
                    slot,
                    path,
                    result,
                } => match result {
                    Ok(image) => {
                        if let Some(slot_state) = self
                            .devices
                            .iter_mut()
                            .find(|d| d.id == device)
                            .and_then(|d| d.slots.get_mut(slot))
                        {
                            if !slot_state
                                .image
                                .as_ref()
                                .is_some_and(|i| i.original_bytes.is_some())
                            {
                                slot_state.image = Some(image);
                                slot_state.error = None;
                            }
                        }
                        self.rebuild_icons(ctx);
                        self.status = format!("VMU {slot} saved to {}", path.display());
                    }
                    Err(error) => self.status = error,
                },
                Event::DumpAll {
                    device,
                    folder,
                    result,
                } => match result {
                    Ok(images) => {
                        if let Some(device) = self.devices.iter_mut().find(|d| d.id == device) {
                            for (slot, image) in images.into_iter().enumerate() {
                                if device.slots[slot]
                                    .image
                                    .as_ref()
                                    .is_some_and(|i| i.original_bytes.is_some())
                                {
                                    continue;
                                }
                                device.slots[slot] = device::SlotState {
                                    image: Some(image),
                                    error: None,
                                };
                            }
                        }
                        self.rebuild_icons(ctx);
                        self.status = format!("Eight VMUs saved to {}", folder.display());
                    }
                    Err(error) => self.status = error,
                },
                Event::Load {
                    device,
                    slot,
                    result,
                } => match result {
                    Ok((backup, image)) => {
                        if let Some(slot_state) = self
                            .devices
                            .iter_mut()
                            .find(|d| d.id == device)
                            .and_then(|d| d.slots.get_mut(slot))
                        {
                            slot_state.image = Some(image);
                            slot_state.error = None;
                        }
                        self.selected_file = None;
                        self.rebuild_icons(ctx);
                        self.status = format!(
                            "VMU {slot} restored and verified. Previous image: {}",
                            ui::display_path(&backup)
                        );
                    }
                    Err(error) => self.status = error,
                },
            }
        }
    }

    fn load_confirmation(&mut self, ctx: &egui::Context) {
        let Some(pending) = &self.pending_load else {
            return;
        };
        let mut confirm = false;
        let mut cancel = false;
        let action = ui::restore_dialog(ctx, pending);
        confirm |= action == ui::RestoreAction::Confirm;
        cancel |= action == ui::RestoreAction::Cancel;
        if cancel {
            self.pending_load = None;
        }
        if confirm {
            self.confirm_load(ctx);
        }
    }
}

impl eframe::App for ManagerApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        native_animation::set_scale(ctx.pixels_per_point());
        self.handle_events(ctx);
        if let Some(music) = &mut self.music {
            music.poll();
        }
        self.keyboard_navigation(ctx);
        ctx.data_mut(|d| d.insert_temp(egui::Id::new("menu-open"), false));
        if self.scenery.is_none() {
            self.scenery = Some(scenery::Scenery::new(ctx));
            self.titlebar = Some(titlebar::TitleBar::new(ctx));
        }
        if ctx.input(|i| i.viewport().close_requested())
            && (self.any_dirty() || self.busy || self.root_editor.is_some())
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.error_popup = Some(if self.busy { "Wait for the current transfer to finish before closing." } else { "There are unsaved VMU edits. Save or discard changes for each edited VMU before closing." }.into());
        }
        if self.native_theme != Some(self.light_theme) {
            native_animation::title_colors(
                frame,
                ctx.style().visuals.panel_fill,
                ctx.style().visuals.text_color(),
                !self.light_theme,
            );
            self.native_theme = Some(self.light_theme);
        }
        let screen = ctx.screen_rect();
        let painter = ctx.layer_painter(egui::LayerId::background());
        painter.rect_filled(screen, 0.0, ctx.style().visuals.panel_fill);
        if let Some(scenery) = &mut self.scenery {
            scenery.background(
                &painter,
                screen,
                self.animation_started.elapsed().as_secs_f64(),
                self.light_theme,
            );
        }
        // Header baseline first, then the transparent branch above it, then UI text.
        painter.hline(
            screen.x_range(),
            screen.top() + titlebar::HEIGHT + 54.0,
            egui::Stroke::new(
                1.0_f32,
                ctx.style().visuals.widgets.noninteractive.bg_stroke.color,
            ),
        );
        if let Some(scenery) = &self.scenery {
            let width = (screen.width() - 400.0).clamp(190.0, 330.0);
            scenery.header(
                &painter,
                egui::Rect::from_min_max(
                    egui::pos2(screen.right() - width, screen.top()),
                    egui::pos2(screen.right(), screen.top() + titlebar::HEIGHT + 78.0),
                ),
            );
        }
        if let Some(titlebar) = &mut self.titlebar {
            titlebar.show(ctx);
        }
        self.menu(ctx);
        egui::CentralPanel::default()
            .frame(egui::Frame::none())
            .show(ctx, |ui| {
                self.workspace(ui);
            });
        self.load_confirmation(ctx);
        if self.root_editor.is_some() {
            let tint = if self.light_theme {
                Color32::from_rgba_unmultiplied(68, 43, 35, 95)
            } else {
                Color32::from_rgba_unmultiplied(7, 3, 5, 165)
            };
            ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("editor-owner-dimmer"),
            ))
            .rect_filled(ctx.screen_rect(), 0.0, tint);
        }
        self.show_root_editor(ctx);
        self.edit_error_dialog(ctx);
        titlebar::resize_edges(ctx);
        ctx.request_repaint_after(std::time::Duration::from_millis(33));
    }
}

#[cfg(test)]
mod app_tests {
    use super::*;

    #[test]
    fn pending_shell_dialog_leaves_egui_running_and_cancellation_preserves_state() {
        let ctx = egui::Context::default();
        let mut app = ManagerApp::create_state(None, "Ready".into());
        app.previews.push((
            "local.bin".into(),
            vmu::parse_image(&vmu::tests::formatted(200)).unwrap(),
        ));
        app.preview_active = true;
        let original = app.current_image().unwrap().bytes.clone();
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let ui_thread = std::thread::current().id();
        app.run_dialog(&ctx, move |_| {
            assert_ne!(ui_thread, std::thread::current().id());
            started_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            Event::OpenImages(None)
        });
        started_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        app.run_dialog(&ctx, |_| panic!("a second dialog must not open"));
        for _ in 0..3 {
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                app.handle_events(ctx);
                assert!(app.busy && app.dialog_open);
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.label("Still repainting");
                });
            });
        }
        release_tx.send(()).unwrap();
        let event = app
            .receiver
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        app.sender.send(event).unwrap();
        app.handle_events(&ctx);
        assert!(!app.busy && !app.dialog_open);
        assert!(app.preview_active);
        assert_eq!(app.current_image().unwrap().bytes, original);
        assert_eq!(app.status, "Ready");
    }

    #[test]
    fn clipboard_and_pending_edits_survive_source_switches_and_discard() {
        let ctx = egui::Context::default();
        let mut app = ManagerApp::create_state(None, String::new());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("source.bin");
        let mut raw = vmu::tests::formatted(200);
        let entry = &mut raw[253 * 512..253 * 512 + 32];
        entry.fill(0);
        entry[0] = 0x33;
        entry[2..4].copy_from_slice(&199u16.to_le_bytes());
        entry[4..8].copy_from_slice(b"SAVE");
        entry[24..26].copy_from_slice(&1u16.to_le_bytes());
        raw[254 * 512 + 398..254 * 512 + 400].copy_from_slice(&0xfffau16.to_le_bytes());
        fs::write(&path, &raw).unwrap();
        app.open_paths(vec![path.clone()], &ctx);
        app.file_action(edits::FileAction::Copy(0), &ctx);
        app.file_action(edits::FileAction::Delete(0), &ctx);
        assert!(app.is_dirty());
        assert!(app.current_image().unwrap().files.is_empty());
        assert_eq!(fs::read(&path).unwrap(), raw, "edits must remain in memory");
        let id = DeviceId {
            bus: Some(1),
            address: Some(2),
        };
        app.devices.push(Device {
            id: id.clone(),
            firmware: vmu::Firmware::MaplePad20,
            slots: vec![device::SlotState {
                image: Some(vmu::parse_image(&vmu::tests::formatted(241)).unwrap()),
                error: None,
            }],
        });
        app.preview_active = false;
        app.file_action(edits::FileAction::Paste(None), &ctx);
        assert!(app.is_dirty());
        assert_eq!(app.current_image().unwrap().files[0].name, "SAVE");
        app.sender
            .send(Event::Dump {
                device: id,
                slot: 0,
                path: "dump.bin".into(),
                result: vmu::parse_image(&vmu::tests::formatted(241)),
            })
            .unwrap();
        app.handle_events(&ctx);
        assert!(app.is_dirty(), "dumping must not discard pending edits");
        app.file_action(edits::FileAction::Discard, &ctx);
        assert!(!app.is_dirty());
        assert!(app.current_image().unwrap().files.is_empty());
        app.close_all_local(&ctx);
        assert_eq!(app.previews.len(), 1, "closing must preserve unsaved edits");
        app.preview_active = true;
        app.file_action(edits::FileAction::Discard, &ctx);
        assert_eq!(app.current_image().unwrap().bytes, raw);
        app.close_all_local(&ctx);
        assert!(app.previews.is_empty());
        assert!(
            app.clipboard.is_some(),
            "clipboard owns its copy independently of the source"
        );
    }

    #[test]
    fn local_images_remain_independent_through_open_select_refresh_and_close() {
        let ctx = egui::Context::default();
        let mut app = ManagerApp::create_state(None, String::new());
        let temp = tempfile::tempdir().unwrap();
        let first = temp.path().join("first.bin");
        let second = temp.path().join("second.bin");
        fs::write(&first, vmu::tests::formatted(200)).unwrap();
        fs::write(&second, vmu::tests::formatted(241)).unwrap();
        app.open_paths(vec![first.clone(), second.clone()], &ctx);
        assert_eq!(app.previews.len(), 2);
        assert_eq!(app.current_image().unwrap().capacity, 241);
        app.sender.send(Event::Scan(Ok(Vec::new()))).unwrap();
        app.handle_events(&ctx);
        assert_eq!(
            app.selected_slot, 1,
            "device refresh must preserve the selected local image"
        );
        app.open_paths(vec![first], &ctx);
        assert_eq!(app.previews.len(), 2, "reopening a path selects/reloads it");
        assert_eq!(app.current_image().unwrap().capacity, 200);
        app.close_local(0, &ctx);
        assert_eq!(app.current_image().unwrap().capacity, 241);
        app.close_local(0, &ctx);
        assert!(app.current_image().is_none());
    }

    #[test]
    fn transfer_result_updates_its_original_maplepad_after_selection_changes() {
        let ctx = egui::Context::default();
        let mut app = ManagerApp::create_state(None, String::new());
        let first = DeviceId {
            bus: Some(1),
            address: Some(2),
        };
        let second = DeviceId {
            bus: Some(1),
            address: Some(3),
        };
        for id in [first.clone(), second] {
            app.devices.push(Device {
                id,
                firmware: vmu::Firmware::MaplePad20,
                slots: vec![device::SlotState {
                    image: None,
                    error: None,
                }],
            });
        }
        app.selected_device = 1;
        app.sender
            .send(Event::Dump {
                device: first,
                slot: 0,
                path: "test.bin".into(),
                result: vmu::parse_image(&vmu::tests::formatted(241)),
            })
            .unwrap();
        app.handle_events(&ctx);
        assert!(app.devices[0].slots[0].image.is_some());
        assert!(app.devices[1].slots[0].image.is_none());
    }
}
