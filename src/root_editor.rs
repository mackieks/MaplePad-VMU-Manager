use super::*;
use crate::{theme::ThemedScrollArea, vmu::root};
use egui::{Color32, Pos2, Rect, Sense, Stroke, Vec2};
use std::io::Write;

#[derive(Clone)]
enum Target {
    Local(PathBuf),
    Device(DeviceId, usize),
}

pub struct Editor {
    bios_texture: Option<(root::Settings, u32, TextureHandle)>,
    native_window: Option<native_animation::EditorWindow>,
    caption: Option<titlebar::TitleBar>,
    target: Target,
    title: String,
    original: VmuImage,
    settings: root::Settings,
    color_enabled: bool,
    mono_enabled: bool,
    color_source: Option<Vec<u8>>,
    mono_source: Option<Vec<u8>>,
    threshold: u8,
    hex: String,
    picker: Hsla,
    textures: HashMap<&'static str, (Vec<u8>, TextureHandle)>,
    pub error: Option<String>,
}
impl Editor {
    fn new(image: VmuImage, target: Target, title: String) -> Self {
        let settings = root::Settings::read(&image);
        let hex = rgba_hex(settings.rgba);
        let picker = Hsla::from_rgba(settings.rgba);
        Self {
            bios_texture: None,
            native_window: None,
            caption: None,
            color_enabled: settings.color.is_some(),
            mono_enabled: settings.mono.is_some(),
            original: image,
            settings,
            target,
            title,
            hex,
            picker,
            color_source: None,
            mono_source: None,
            threshold: 128,
            textures: HashMap::new(),
            error: None,
        }
    }
    pub fn import(&mut self, rgba: Vec<u8>, mono: bool) {
        if mono {
            self.settings.mono = Some(root::monochrome(&rgba, self.threshold));
            self.mono_source = Some(rgba);
            self.mono_enabled = true;
        } else {
            self.settings.color = Some(root::optimize(&rgba));
            self.color_source = Some(rgba);
            self.color_enabled = true;
        }
        self.error = None;
    }
    fn pending(&self) -> root::Settings {
        let mut settings = self.settings.clone();
        if !self.color_enabled {
            settings.color = None;
        }
        if !self.mono_enabled {
            settings.mono = None;
        }
        settings
    }
    fn texture(&mut self, ctx: &egui::Context, name: &'static str, rgba: &[u8]) -> TextureHandle {
        let entry = self.textures.entry(name).or_insert_with(|| {
            (
                Vec::new(),
                ctx.load_texture(
                    name,
                    egui::ColorImage::new([32, 32], Color32::TRANSPARENT),
                    egui::TextureOptions::NEAREST,
                ),
            )
        });
        if entry.0 != rgba {
            entry.1.set(
                egui::ColorImage::from_rgba_unmultiplied([32, 32], rgba),
                egui::TextureOptions::NEAREST,
            );
            entry.0 = rgba.to_vec();
        }
        entry.1.clone()
    }
    fn preview(
        &mut self,
        ui: &mut egui::Ui,
        name: &'static str,
        rgba: &[u8],
        scale: f32,
        lcd: bool,
    ) {
        let texture = self.texture(ui.ctx(), name, rgba);
        let ppp = ui.ctx().pixels_per_point();
        let size = 32.0 * (scale * ppp).floor().max(1.0) / ppp;
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
        let rect = Rect::from_min_size(ui.painter().round_pos_to_pixels(rect.min), rect.size());
        if lcd {
            draw_lcd(ui, rect, rgba);
            return;
        } else {
            checker(ui, rect);
        }
        ui.painter().image(
            texture.id(),
            rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );
    }
    fn bios_preview(&mut self, ui: &mut egui::Ui, settings: &root::Settings) {
        let ppp = ui.ctx().pixels_per_point();
        let scale = crate::bios_preview::pixel_scale(
            settings.real_mode,
            (ui.available_width() - 20.0).max(32.0) * ppp,
            280.0 * ppp,
        );
        if !self
            .bios_texture
            .as_ref()
            .is_some_and(|(old, s, _)| old == settings && *s == scale)
        {
            let image = crate::bios_preview::render(settings, scale);
            let texture = ui.ctx().load_texture(
                "bios-body-preview",
                egui::ColorImage::from_rgba_unmultiplied(
                    [image.width() as usize, image.height() as usize],
                    image.as_raw(),
                ),
                egui::TextureOptions::NEAREST,
            );
            self.bios_texture = Some((settings.clone(), scale, texture));
        }
        let texture = &self.bios_texture.as_ref().unwrap().2;
        let size = texture.size_vec2() / ppp;
        let (bounds, _) = ui.allocate_exact_size(size + Vec2::splat(20.0), Sense::hover());
        ui.painter().rect_filled(
            bounds,
            2.0,
            if settings.real_mode {
                Color32::from_rgb(0, 103, 149)
            } else {
                Color32::from_rgb(115, 147, 199)
            },
        );
        let rect = Rect::from_min_size(
            ui.painter()
                .round_pos_to_pixels(bounds.min + Vec2::splat(10.0)),
            size,
        );
        ui.painter().image(
            texture.id(),
            rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );
    }
    fn show(&mut self, ctx: &egui::Context, waiting: bool) -> Action {
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("root-editor-window"),
            egui::ViewportBuilder::default()
                .with_title("Edit Root Block")
                .with_inner_size([730.0, 740.0])
                .with_min_inner_size([650.0, 540.0])
                .with_taskbar(false)
                .with_decorations(false),
            |ctx, _class| self.show_contents(ctx, waiting),
        )
    }
    fn show_contents(&mut self, ctx: &egui::Context, waiting: bool) -> Action {
        if self.native_window.is_none() && ctx.input(|i| i.viewport().focused.unwrap_or(false)) {
            self.native_window = native_animation::install_editor(ctx);
        }
        let mut action = Action::None;
        ctx.layer_painter(egui::LayerId::background()).rect_filled(
            ctx.screen_rect(),
            2.0,
            ctx.style().visuals.panel_fill,
        );
        let caption = self
            .caption
            .get_or_insert_with(|| titlebar::TitleBar::new(ctx));
        let close = caption.show_named(ctx, "Edit Root Block", false)
            || ctx.input(|i| i.viewport().close_requested());
        if close && waiting {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::none().inner_margin(10.0))
            .show(ctx,|ui| {
                ui.add_enabled_ui(!waiting,|ui| {
                    ui.label(egui::RichText::new(&self.title).strong());
                    ui.label(egui::RichText::new(format!("Current image: {} blocks · {} free", self.original.capacity,self.original.free)).small());
                    ui.horizontal(|ui| {
                        ui.label("Format");
                        ui.add_enabled_ui(matches!(self.target, Target::Local(_)), |ui| {
                            egui::ComboBox::from_id_salt("root-format")
                                .selected_text(if self.settings.capacity == 200 { "MaplePad 1.5 · 200 blocks" } else { "MaplePad 2.0 · 241 blocks" })
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut self.settings.capacity, 200, "MaplePad 1.5 · 200 blocks");
                                    ui.selectable_value(&mut self.settings.capacity, 241, "MaplePad 2.0 · 241 blocks");
                                });
                        }).response.on_disabled_hover_text("Connected VMUs use the installed MaplePad firmware's format. Dump the VMU to convert a local image.");
                    });
                    ui.separator();
                    egui::ScrollArea::vertical().id_salt("root-content")
                        .max_height((ctx.screen_rect().height()-200.0).max(280.0)).show_themed(ui,|ui| {
                        ui.horizontal_top(|ui| {
                            ui.vertical(|ui| {
                                ui.set_width(390.0);
                                ui.checkbox(&mut self.settings.custom_color,"Custom VMU color");
                                ui.add_enabled_ui(self.settings.custom_color,|ui| {
                                    if color_picker(ui,&mut self.settings.rgba,&mut self.picker) { self.hex=rgba_hex(self.settings.rgba); }
                                    ui.horizontal(|ui| {
                                        ui.label("RGBA");
                                        if ui.add(egui::TextEdit::singleline(&mut self.hex).desired_width(100.0).char_limit(9)).changed() {
                                            if let Some(rgba)=parse_hex(&self.hex) { self.settings.rgba=rgba; }
                                        }
                                        ui.label(egui::RichText::new("#RRGGBBAA").small());
                                    });
                                });
                                ui.add_space(6.0);
                                ui.horizontal(|ui| {
                                    ui.label("BIOS icon");
                                    ui.add(egui::DragValue::new(&mut self.settings.shape).range(0..=123).speed(0.2));
                                    if ui.small_button("‹").clicked() { self.settings.shape=self.settings.shape.saturating_sub(1); }
                                    if ui.small_button("›").clicked() { self.settings.shape=(self.settings.shape+1).min(123); }
                                    ui.label("0–123");
                                });
                                ui.checkbox(&mut self.settings.real_mode,"Real Mode").on_hover_text("Enable the Dreamcast BIOS's hidden 3D menu using ICONDATA_VMS.");
                                ui.separator();
                                ui.horizontal(|ui| {
                                    ui.checkbox(&mut self.color_enabled,"Custom color icon");
                                    if ui.button("Import image…").clicked() { action=Action::Import(false); }
                                });
                                if self.color_enabled {
                                    if self.settings.color.is_none() { ui.label("Import a PNG, JPG, or BMP image."); }
                                    else {
                                        ui.label(egui::RichText::new("32 × 32 · optimized 16-color ARGB4444 palette").small());
                                        if let Some(color)=self.settings.color.clone() {
                                            ui.horizontal_top(|ui| {
                                                self.preview(ui,"converted-color",&color.rgba(),2.0,false);
                                                ui.add_space(8.0);
                                                ui.vertical(|ui| { ui.label("Palette"); palette(ui,&color.palette); });
                                            });
                                        }
                                    }
                                }
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    ui.checkbox(&mut self.mono_enabled,"Custom mono LCD icon");
                                    if ui.button("Import image…").clicked() { action=Action::Import(true); }
                                });
                                if self.mono_enabled {
                                    if let Some(source)=&self.mono_source {
                                        if ui.add(egui::Slider::new(&mut self.threshold,1..=254).text("Black threshold")).changed() {
                                            self.settings.mono=Some(root::monochrome(source,self.threshold));
                                        }
                                    }
                                    if let Some(bits)=self.settings.mono {
                                        self.preview(ui,"converted-mono",&root::mono_rgba(&bits,[0,0,0,255]),2.0,true);
                                        ui.label(egui::RichText::new("32 × 32 · 1-bit indexed · black / transparent").small());
                                    } else { ui.label("Import a PNG, JPG, or BMP image."); }
                                }
                                if self.color_enabled || self.mono_enabled || self.settings.real_mode {
                                    ui.horizontal(|ui| {
                                        ui.label("Icon description");
                                        ui.add(egui::TextEdit::singleline(&mut self.settings.description).desired_width(170.0));
                                    });
                                }
                            });
                            ui.separator();
                            ui.vertical(|ui| {
                                ui.set_min_width(170.0);
                                ui.label(egui::RichText::new("Dreamcast BIOS preview").strong());
                                ui.add_space(8.0);
                                let settings=self.pending();
                                let color=if settings.custom_color { settings.rgba } else { [255;4] };
                                self.bios_preview(ui,&settings);
                                ui.add_space(6.0);
                                ui.label(format!("BIOS icon #{}",settings.shape));
                                if settings.color.is_some() || settings.mono.is_some() { ui.label(egui::RichText::new("Custom icon overrides the BIOS icon.").small()); }
                                ui.horizontal(|ui| {
                                    ui.label("VMU color");
                                    let (r,_)=ui.allocate_exact_size(egui::vec2(42.0,20.0),Sense::hover());
                                    checker(ui,r);ui.painter().rect_filled(r,2.0,Color32::from_rgba_unmultiplied(color[0],color[1],color[2],color[3]));
                                });
                                ui.add_space(8.0);
                                ui.label("LCD preview");
                                let mono=settings.mono.unwrap_or_else(||root::bios_mono(settings.shape));
                                self.preview(ui,"lcd-preview",&root::mono_rgba(&mono,[0,0,0,255]),4.0,true);
                                ui.add_space(8.0);
                                ui.label(format!("Icon data: {} block(s)",settings.icon_blocks()));
                                if settings.real_mode { ui.label("Real Mode enabled"); }
                            });
                        });
                    });
                    ui.separator();
                    ui.label(egui::RichText::new("Save adds pending changes. Use Save Changes to write the VMU.").small());
                    if let Some(error)=&self.error { ui.colored_label(ui.visuals().selection.stroke.color,error); }
                    let valid=(!self.settings.custom_color || parse_hex(&self.hex).is_some())
                        && (!self.color_enabled || self.settings.color.is_some())
                        && (!self.mono_enabled || self.settings.mono.is_some());
                    ui.horizontal(|ui| {
                        if ui.add_enabled(valid,egui::Button::new("Save").fill(ui.visuals().selection.bg_fill)).clicked() { action=Action::Save; }
                        if ui.button("Cancel").clicked() { action=Action::Cancel; }
                    });
                });
            });
        titlebar::resize_edges(ctx);
        if !waiting && (close || ctx.input(|i| i.key_pressed(egui::Key::Escape))) {
            action = Action::Cancel;
        }
        action
    }
}
enum Action {
    None,
    Import(bool),
    Save,
    Cancel,
}
impl Drop for Editor {
    fn drop(&mut self) {
        if let Some(window) = self.native_window.take() {
            native_animation::release_editor(window);
        }
    }
}

impl ManagerApp {
    /// A confirmed Save As can replace an already open image, including one
    /// with pending edits. Keep its tab but refresh from the new disk contents.
    pub(super) fn refresh_replaced_local(&mut self, path: &std::path::Path, ctx: &egui::Context) {
        let path = fs::canonicalize(path).unwrap_or_else(|_| path.to_owned());
        if let Some(index) = self.previews.iter().position(|(p, _)| *p == path) {
            let key = self.selected_file_key();
            match fs::read(&path)
                .map_err(|e| e.to_string())
                .and_then(|bytes| vmu::parse_image(&bytes))
            {
                Ok(image) => {
                    self.previews[index].1 = image;
                    self.refresh_saved_view(ctx, key);
                }
                Err(error) => self.error_popup = Some(error),
            }
        }
    }
    pub(super) fn open_root_editor(&mut self) {
        if self.busy {
            return;
        }
        let Some(image) = self.current_image().cloned() else {
            return;
        };
        let (target, title) = if self.preview_active {
            let path = self.previews[self.selected_slot].0.clone();
            let title = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            (Target::Local(path), title)
        } else {
            let Some(device) = self.current_device_id() else {
                return;
            };
            (
                Target::Device(device, self.selected_slot),
                format!("MaplePad · VMU {}", self.selected_slot),
            )
        };
        self.root_editor = Some(Editor::new(image, target, title));
    }
    pub(super) fn show_root_editor(&mut self, ctx: &egui::Context) {
        let Some(mut editor) = self.root_editor.take() else {
            return;
        };
        let action = editor.show(ctx, self.busy);
        match action {
            Action::Cancel => return,
            Action::Import(mono) => {
                let parent = editor.native_window;
                self.run_dialog(ctx, move |dialog| {
                    let dialog = if let Some(parent) = &parent {
                        dialog.set_parent(parent)
                    } else {
                        dialog
                    };
                    let result = dialog
                        .add_filter("Icon image", &["png", "jpg", "jpeg", "bmp"])
                        .pick_file()
                        .map(|path| root::import_image(&path))
                        .transpose();
                    Event::RootImported { mono, result }
                })
            }
            Action::Save => {
                match root::apply(&editor.original, &editor.pending()) {
                    Ok(updated) => {
                        let key = self.selected_file_key();
                        let target = match &editor.target {
                            Target::Local(path) => self
                                .previews
                                .iter_mut()
                                .find(|(p, _)| p == path)
                                .map(|(_, i)| i),
                            Target::Device(id, slot) => self
                                .devices
                                .iter_mut()
                                .find(|d| &d.id == id)
                                .and_then(|d| d.slots.get_mut(*slot))
                                .and_then(|s| s.image.as_mut()),
                        };
                        if let Some(target) = target.filter(|i| i.bytes == editor.original.bytes) {
                            *target = updated;
                            self.refresh_saved_view(ctx, key);
                            self.status =
                                "Root settings updated · choose Save Changes or Discard Changes"
                                    .into();
                            return;
                        }
                        editor.error=Some("This VMU changed while the editor was open. Cancel and reopen the editor.".into());
                    }
                    Err(e) => editor.error = Some(e),
                }
            }
            Action::None => {}
        }
        self.root_editor = Some(editor);
    }
    pub(super) fn create_new_vmu(&mut self, ctx: &egui::Context) {
        self.run_dialog(ctx, |dialog| {
            let result = dialog
                .add_filter("VMU image", &["bin"])
                .set_file_name("new-vmu.bin")
                .save_file()
                .map(|path| {
                    let path = bin_path(path);
                    write_new(&path, &root::empty_image().bytes)?;
                    Ok(path)
                })
                .transpose();
            Event::Created(result)
        });
    }
    pub(super) fn save_copy_as(&mut self, slot: usize, ctx: &egui::Context) {
        let Some((source, image)) = self.previews.get(slot) else {
            return;
        };
        let name = format!(
            "{}-copy.bin",
            source.file_stem().unwrap_or_default().to_string_lossy()
        );
        let bytes = if image.format == vmu::ImageFormat::Native20 {
            vmu::swap_words(&image.bytes)
        } else {
            image.bytes.clone()
        };
        self.run_dialog(ctx, move |dialog| {
            let result = dialog
                .add_filter("VMU image", &["bin"])
                .set_file_name(name)
                .save_file()
                .map(|path| {
                    let path = bin_path(path);
                    write_new(&path, &bytes)?;
                    Ok(path)
                })
                .transpose();
            Event::CopySaved(result)
        });
    }
}
fn bin_path(mut path: PathBuf) -> PathBuf {
    if path.extension().is_none() {
        path.set_extension("bin");
    }
    path
}
fn write_new(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    let mut file =
        tempfile::NamedTempFile::new_in(path.parent().ok_or("Missing destination folder")?)
            .map_err(|e| e.to_string())?;
    file.write_all(bytes)
        .and_then(|_| file.as_file().sync_all())
        .map_err(|e| e.to_string())?;
    // The Windows Save dialog already obtained overwrite confirmation. Replace
    // atomically, retaining the old file if writing the temporary file fails.
    file.persist(path)
        .map_err(|e| format!("Cannot save image: {e}"))?;
    if fs::read(path).map_err(|e| e.to_string())? != bytes {
        return Err("Saved image did not match its read-back.".into());
    }
    Ok(())
}
fn rgba_hex(rgba: [u8; 4]) -> String {
    format!(
        "#{:02X}{:02X}{:02X}{:02X}",
        rgba[0], rgba[1], rgba[2], rgba[3]
    )
}
fn parse_hex(value: &str) -> Option<[u8; 4]> {
    let hex = value.trim().trim_start_matches('#');
    if hex.len() != 8 || !hex.is_ascii() {
        return None;
    }
    let value = u32::from_str_radix(hex, 16).ok()?;
    Some(value.to_be_bytes())
}
fn checker(ui: &egui::Ui, rect: Rect) {
    let p = ui.painter().with_clip_rect(ui.clip_rect().intersect(rect));
    for y in 0..(rect.height() / 8.0).ceil() as usize {
        for x in 0..(rect.width() / 8.0).ceil() as usize {
            p.rect_filled(
                Rect::from_min_size(
                    rect.min + egui::vec2(x as f32 * 8.0, y as f32 * 8.0),
                    Vec2::splat(8.0),
                ),
                0.0,
                Color32::from_gray(if (x + y) % 2 == 0 { 180 } else { 220 }),
            );
        }
    }
}
fn palette(ui: &mut egui::Ui, colors: &[[u8; 4]; 16]) {
    let (bounds, _) = ui.allocate_exact_size(Vec2::splat(66.0), Sense::hover());
    for (index, c) in colors.iter().enumerate() {
        let r = Rect::from_min_size(
            bounds.min + egui::vec2((index % 4) as f32 * 17.0, (index / 4) as f32 * 17.0),
            Vec2::splat(15.0),
        );
        let response = ui.interact(r, ui.id().with(("palette", index)), Sense::hover());
        checker(ui, r);
        ui.painter().rect_filled(
            r,
            0.0,
            Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]),
        );
        response.on_hover_text(rgba_hex(*c));
    }
}
fn draw_lcd(ui: &egui::Ui, rect: Rect, rgba: &[u8]) {
    let p = ui.painter().with_clip_rect(rect);
    p.rect_filled(rect, 2.0, Color32::from_rgb(68, 118, 102));
    let pitch = rect.width() / 32.0;
    for y in 0..32 {
        for x in 0..32 {
            let pos = rect.min + egui::vec2(x as f32 * pitch, y as f32 * pitch);
            let glass = 10.0 * (1.0 - x as f32 / 31.0) + 5.0 * (1.0 - y as f32 / 31.0);
            let grain = ((x * 17 + y * 31) % 7) as f32 - 3.0;
            let color = |base: [f32; 3], amount: f32| {
                Color32::from_rgb(
                    (base[0] + amount).clamp(0.0, 255.0) as u8,
                    (base[1] + amount).clamp(0.0, 255.0) as u8,
                    (base[2] + amount).clamp(0.0, 255.0) as u8,
                )
            };
            let cell = Rect::from_min_size(pos, Vec2::splat(pitch));
            p.rect_filled(cell, 0.0, color([72.0, 133.0, 111.0], glass + grain * 0.35));
        }
    }
    // Draw substrate, then all shadows, then electrodes: a later cell's
    // background must not erase its neighbour's offset shadow.
    let gap = 1.0 / ui.ctx().pixels_per_point();
    let dot_rect = |x: usize, y: usize| {
        Rect::from_min_size(
            rect.min + egui::vec2(x as f32 * pitch, y as f32 * pitch),
            Vec2::splat(pitch),
        )
        .shrink(gap * 0.5)
    };
    for y in 0..32 {
        for x in 0..32 {
            if rgba[(y * 32 + x) * 4 + 3] > 127 {
                let shadow = dot_rect(x, y).translate(egui::vec2(gap * 0.65, gap * 0.8));
                p.rect_filled(
                    shadow.expand(gap * 0.3),
                    0.0,
                    Color32::from_rgba_unmultiplied(9, 34, 35, 20),
                );
                p.rect_filled(shadow, 0.0, Color32::from_rgba_unmultiplied(9, 34, 35, 38));
            }
        }
    }
    for y in 0..32 {
        for x in 0..32 {
            let active = rgba[(y * 32 + x) * 4 + 3] > 127;
            let glass = 10.0 * (1.0 - x as f32 / 31.0) + 5.0 * (1.0 - y as f32 / 31.0);
            let grain = ((x * 17 + y * 31) % 7) as f32 - 3.0;
            let base = if active {
                [10.0, 37.0, 62.0]
            } else {
                [69.0, 125.0, 107.0]
            };
            let amount = glass * if active { 0.35 } else { 0.85 } + grain * 0.3;
            let color = base.map(|c| (c + amount).clamp(0.0, 255.0) as u8);
            p.rect_filled(
                dot_rect(x, y),
                0.0,
                Color32::from_rgb(color[0], color[1], color[2]),
            );
        }
    }
    p.rect_stroke(
        rect.shrink(0.5),
        2.0,
        Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(25, 58, 46, 150)),
    );
}

#[derive(Clone, Copy)]
struct Hsla {
    h: f32,
    s: f32,
    l: f32,
    a: f32,
}
impl Hsla {
    fn from_rgba(rgba: [u8; 4]) -> Self {
        let [r, g, b, a] = rgba.map(|v| v as f32 / 255.0);
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let d = max - min;
        let l = (max + min) * 0.5;
        let h = if d == 0.0 {
            0.0
        } else if max == r {
            ((g - b) / d).rem_euclid(6.0) / 6.0
        } else if max == g {
            ((b - r) / d + 2.0) / 6.0
        } else {
            ((r - g) / d + 4.0) / 6.0
        };
        let s = if d == 0.0 {
            0.0
        } else {
            d / (1.0 - (2.0 * l - 1.0).abs())
        };
        Self { h, s, l, a }
    }
    fn rgba(self) -> [u8; 4] {
        let c = (1.0 - (2.0 * self.l - 1.0).abs()) * self.s;
        let h = self.h.rem_euclid(1.0) * 6.0;
        let x = c * (1.0 - (h % 2.0 - 1.0).abs());
        let m = self.l - c / 2.0;
        let [r, g, b] = match h as u32 {
            0 => [c, x, 0.0],
            1 => [x, c, 0.0],
            2 => [0.0, c, x],
            3 => [0.0, x, c],
            4 => [x, 0.0, c],
            _ => [c, 0.0, x],
        };
        [r + m, g + m, b + m, self.a].map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
    }
    fn color(self) -> Color32 {
        let [r, g, b, a] = self.rgba();
        Color32::from_rgba_unmultiplied(r, g, b, a)
    }
}
fn color_picker(ui: &mut egui::Ui, rgba: &mut [u8; 4], hsl: &mut Hsla) -> bool {
    let before = *rgba;
    let mut interacted = false;
    // Retain hue/saturation at black and white, where RGB cannot encode them.
    // Resynchronize when the hex field or an external source changes the color.
    if hsl.rgba() != *rgba {
        *hsl = Hsla::from_rgba(*rgba);
    }
    ui.horizontal(|ui| {
        let (rect, response) = ui.allocate_exact_size(Vec2::splat(158.0), Sense::click_and_drag());
        let radius = 74.0;
        let center = rect.center();
        if let Some(pos) = response.interact_pointer_pos() {
            interacted = true;
            let delta = pos - center;
            hsl.h = (delta.y.atan2(delta.x) / std::f32::consts::TAU).rem_euclid(1.0);
            hsl.s = (delta.length() / radius).min(1.0);
        }
        let opaque = |h, s, l| Hsla { h, s, l, a: 1.0 }.color();
        let mut mesh = egui::Mesh::default();
        mesh.colored_vertex(center, opaque(hsl.h, 0.0, hsl.l));
        for i in 0..=96 {
            let hue = i as f32 / 96.0;
            let angle = hue * std::f32::consts::TAU;
            mesh.colored_vertex(
                center + egui::vec2(angle.cos(), angle.sin()) * radius,
                opaque(hue, 1.0, hsl.l),
            );
            if i > 0 {
                mesh.add_triangle(0, i, i + 1);
            }
        }
        ui.painter().add(mesh);
        let angle = hsl.h * std::f32::consts::TAU;
        let marker = center + egui::vec2(angle.cos(), angle.sin()) * hsl.s * radius;
        ui.painter()
            .circle_stroke(marker, 4.0, Stroke::new(2.0_f32, Color32::BLACK));
        ui.painter()
            .circle_stroke(marker, 3.0, Stroke::new(1.0_f32, Color32::WHITE));
        for alpha in [false, true] {
            ui.vertical(|ui| {
                ui.label(if alpha { "Alpha" } else { "Lightness" });
                let (r, response) =
                    ui.allocate_exact_size(egui::vec2(22.0, 128.0), Sense::click_and_drag());
                if let Some(pos) = response.interact_pointer_pos() {
                    interacted = true;
                    let value = (1.0 - (pos.y - r.top()) / r.height()).clamp(0.0, 1.0);
                    if alpha {
                        hsl.a = value;
                    } else {
                        hsl.l = value;
                    }
                }
                checker(ui, r);
                for y in 0..128 {
                    let value = 1.0 - y as f32 / 127.0;
                    let color = Hsla {
                        h: hsl.h,
                        s: hsl.s,
                        l: if alpha { hsl.l } else { value },
                        a: if alpha { value } else { 1.0 },
                    }
                    .color();
                    ui.painter().rect_filled(
                        Rect::from_min_size(
                            r.min + egui::vec2(0.0, y as f32),
                            egui::vec2(r.width(), 1.0),
                        ),
                        0.0,
                        color,
                    );
                }
                let y = r.bottom() - (if alpha { hsl.a } else { hsl.l }) * r.height();
                ui.painter()
                    .hline(r.x_range(), y, Stroke::new(3.0_f32, Color32::BLACK));
                ui.painter()
                    .hline(r.x_range(), y, Stroke::new(1.0_f32, Color32::WHITE));
            });
        }
    });
    // Avoid changing bytes merely by opening the editor (HSL round-trip).
    let converted = hsl.rgba();
    if interacted && converted != before {
        *rgba = converted;
    }
    *rgba != before
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hsl_picker_roundtrips_srgb_and_lightness_reaches_both_endpoints() {
        for r in [0, 1, 64, 128, 200, 255] {
            for g in [0, 1, 64, 128, 200, 255] {
                for b in [0, 1, 64, 128, 200, 255] {
                    let rgba = [r, g, b, 117];
                    assert_eq!(Hsla::from_rgba(rgba).rgba(), rgba);
                }
            }
        }
        let mut color = Hsla {
            h: 0.0,
            s: 1.0,
            l: 0.5,
            a: 1.0,
        };
        assert_eq!(color.rgba(), [255, 0, 0, 255]);
        color.l = 0.0;
        assert_eq!(color.rgba(), [0, 0, 0, 255]);
        color.l = 1.0;
        assert_eq!(color.rgba(), [255, 255, 255, 255]);
    }
    #[test]
    fn editor_close_request_cancels_without_changing_image() {
        let ctx = egui::Context::default();
        let image = root::empty_image();
        let mut editor = Editor::new(
            image.clone(),
            Target::Local("test.bin".into()),
            "test.bin".into(),
        );
        editor.settings.rgba = [12, 34, 56, 78];
        let mut input = egui::RawInput::default();
        input
            .viewports
            .get_mut(&egui::ViewportId::ROOT)
            .unwrap()
            .events
            .push(egui::ViewportEvent::Close);
        let mut action = Action::None;
        let _ = ctx.run(input, |ctx| {
            action = editor.show_contents(ctx, false);
        });
        assert!(matches!(action, Action::Cancel));
        assert_eq!(editor.original.bytes, image.bytes);
    }
    #[test]
    fn editor_caption_x_click_cancels() {
        let ctx = egui::Context::default();
        let mut editor = Editor::new(
            root::empty_image(),
            Target::Local("test.bin".into()),
            "test.bin".into(),
        );
        let position = egui::pos2(707.0, 15.0);
        let mut action = Action::None;
        for pressed in [None, Some(true), Some(false)] {
            let mut input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(730.0, 740.0))),
                ..Default::default()
            };
            input.events.push(egui::Event::PointerMoved(position));
            if let Some(pressed) = pressed {
                input.events.push(egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                });
            }
            let _ = ctx.run(input, |ctx| {
                action = editor.show_contents(ctx, false);
            });
        }
        assert!(matches!(action, Action::Cancel));
    }
    #[test]
    fn overwritten_open_image_refreshes_pending_model() {
        let ctx = egui::Context::default();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("new-vmu.bin");
        let image = root::empty_image();
        write_new(&path, &image.bytes).unwrap();
        let mut app = ManagerApp::create_state(None, String::new());
        app.open_paths(vec![path.clone()], &ctx);
        let mut settings = root::Settings::read(&image);
        settings.capacity = 241;
        app.previews[0].1 = root::apply(&image, &settings).unwrap();
        write_new(&path, &image.bytes).unwrap();
        app.refresh_replaced_local(&path, &ctx);
        assert_eq!(app.previews.len(), 1);
        assert_eq!(app.current_image().unwrap().capacity, 200);
        assert!(!app.is_dirty());
    }
    #[test]
    fn creating_and_copying_images_replace_confirmed_destinations() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("new.bin");
        let image = root::empty_image();
        write_new(&path, &image.bytes).unwrap();
        assert_eq!(
            vmu::parse_image(&fs::read(&path).unwrap()).unwrap().free,
            200
        );
        let mut settings = root::Settings::read(&image);
        settings.capacity = 241;
        let replacement = root::apply(&image, &settings).unwrap();
        write_new(&path, &replacement.bytes).unwrap();
        assert_eq!(fs::read(&path).unwrap(), replacement.bytes);
        let copy = dir.path().join("renamed.bin");
        write_new(&copy, &replacement.bytes).unwrap();
        assert_eq!(fs::read(copy).unwrap(), fs::read(path).unwrap());
    }
    #[test]
    fn imports_support_png_jpg_bmp_and_fit_without_stretching() {
        let dir = tempfile::tempdir().unwrap();
        for ext in ["png", "jpg", "bmp"] {
            let path = dir.path().join(format!("icon.{ext}"));
            image::RgbImage::from_pixel(64, 32, image::Rgb([230, 30, 40]))
                .save(&path)
                .unwrap();
            let rgba = root::import_image(&path).unwrap();
            assert_eq!(rgba.len(), 4096);
            assert!(rgba[..32 * 8 * 4].iter().all(|&b| b == 0));
            assert_eq!(rgba[(16 * 32 + 16) * 4 + 3], 255);
        }
    }
    #[test]
    fn format_conversion_discard_restores_format_and_bytes() {
        let ctx = egui::Context::default();
        let mut app = ManagerApp::create_state(None, String::new());
        let image = root::empty_image();
        let original = image.bytes.clone();
        app.previews.push(("local.bin".into(), image));
        app.preview_active = true;
        let mut settings = root::Settings::read(app.current_image().unwrap());
        settings.capacity = 241;
        app.previews[0].1 = root::apply(app.current_image().unwrap(), &settings).unwrap();
        app.file_action(crate::edits::FileAction::Discard, &ctx);
        assert_eq!(
            app.current_image().unwrap().format,
            vmu::ImageFormat::MaplePad15
        );
        assert_eq!(app.current_image().unwrap().bytes, original);
        assert!(!app.is_dirty());
    }
}
