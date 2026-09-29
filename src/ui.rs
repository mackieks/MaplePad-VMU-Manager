use super::*;
use crate::edits::FileAction;
use crate::theme::ThemedScrollArea;
use egui::{Align2, FontFamily, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use std::time::Duration;

const RED: Color32 = Color32::from_rgb(205, 24, 43);
fn menu_button<R>(ui: &mut egui::Ui, title: &str, content: impl FnOnce(&mut egui::Ui) -> R) {
    ui.menu_button(title, |ui| scroll_menu(ui, content));
}
fn mark_menu_open(ctx: &egui::Context) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("menu-open"), true));
}
fn scroll_menu<R>(ui: &mut egui::Ui, content: impl FnOnce(&mut egui::Ui) -> R) -> R {
    mark_menu_open(ui.ctx());
    ui.spacing_mut().button_padding.x = 6.0;
    egui::ScrollArea::vertical()
        .id_salt("menu-scroll")
        .max_height((ui.ctx().screen_rect().height() - 24.0).clamp(80.0, 360.0))
        .show_themed(ui, content)
        .inner
}

fn selection_fill(ui: &egui::Ui) -> Color32 {
    ui.visuals().selection.bg_fill
}
fn accent(ui: &egui::Ui) -> Color32 {
    if ui.visuals().dark_mode {
        Color32::from_rgb(242, 70, 88)
    } else {
        Color32::from_rgb(150, 44, 43)
    }
}
fn text(ui: &egui::Ui) -> Color32 {
    ui.visuals().text_color()
}
fn muted(ui: &egui::Ui) -> Color32 {
    if ui.visuals().dark_mode {
        Color32::from_rgb(155, 155, 162)
    } else {
        Color32::from_rgb(108, 88, 74)
    }
}
fn rule_color(ui: &egui::Ui) -> Color32 {
    ui.visuals().widgets.noninteractive.bg_stroke.color
}
fn stripe(ui: &egui::Ui) -> Color32 {
    if ui.visuals().dark_mode {
        Color32::from_rgba_unmultiplied(40, 40, 44, 85)
    } else {
        Color32::from_rgba_unmultiplied(155, 122, 84, 22)
    }
}
fn selected_text(selected: bool, ui: &egui::Ui) -> Color32 {
    if selected {
        if ui.visuals().dark_mode {
            Color32::WHITE
        } else {
            ui.visuals().selection.stroke.color
        }
    } else {
        text(ui)
    }
}
const EMPTY_MESSAGE: &str = "Connect MaplePad in BOOTSEL mode or open a local VMU image";

enum CardAction {
    SaveCopy(usize),
    Root(bool, usize),
    Reveal(PathBuf),
    Close(usize),
    Dump(usize),
    Load(usize),
}

fn file_action_buttons(
    ui: &mut egui::Ui,
    selected: Option<usize>,
    dirty: bool,
    clipboard: bool,
    enabled: bool,
    moves: Option<(bool, bool, bool)>,
    action: &mut Option<FileAction>,
) {
    // Each button participates in the parent wrapping layout individually.
    if let (Some(index), Some((up, down, up_is_higher))) = (selected, moves) {
        if ui
            .add_enabled(enabled && up, egui::Button::new("Move ⬆"))
            .clicked()
        {
            *action = Some(FileAction::Move(index, up_is_higher));
        }
        if ui
            .add_enabled(enabled && down, egui::Button::new("Move ⬇"))
            .clicked()
        {
            *action = Some(FileAction::Move(index, !up_is_higher));
        }
    }
    if let Some(index) = selected {
        if ui.add_enabled(enabled, egui::Button::new("Copy")).clicked() {
            *action = Some(FileAction::Copy(index));
        }
    }
    if clipboard
        && ui
            .add_enabled(enabled, egui::Button::new("Paste"))
            .clicked()
    {
        *action = Some(FileAction::Paste(None));
    }
    if let Some(index) = selected {
        if ui
            .add_enabled(enabled, egui::Button::new("Delete"))
            .clicked()
        {
            *action = Some(FileAction::Delete(index));
        }
    }
    if dirty {
        if ui
            .add_enabled(
                enabled,
                egui::Button::new(RichText::new("Save Changes").color(selected_text(true, ui)))
                    .fill(selection_fill(ui)),
            )
            .clicked()
        {
            *action = Some(FileAction::Save);
        }
        if ui
            .add_enabled(enabled, egui::Button::new("Discard Changes"))
            .clicked()
        {
            *action = Some(FileAction::Discard);
        }
    }
}

fn save_context(
    response: &egui::Response,
    file: Option<usize>,
    clipboard: bool,
    enabled: bool,
    action: &mut Option<FileAction>,
) {
    response.context_menu(|ui| {
        scroll_menu(ui, |ui| {
            if let Some(index) = file {
                if ui
                    .add_enabled(enabled, egui::Button::new("Copy File"))
                    .clicked()
                {
                    *action = Some(FileAction::Copy(index));
                    ui.close_menu();
                }
            }
            if ui
                .add_enabled(enabled && clipboard, egui::Button::new("Paste File"))
                .clicked()
            {
                *action = Some(FileAction::Paste(file));
                ui.close_menu();
            }
            if let Some(index) = file {
                if ui
                    .add_enabled(enabled, egui::Button::new("Delete File"))
                    .clicked()
                {
                    *action = Some(FileAction::Delete(index));
                    ui.close_menu();
                }
            }
        });
    });
}

pub(crate) fn display_path(path: &std::path::Path) -> String {
    let text = path.to_string_lossy();
    if let Some(unc) = text.strip_prefix("\\\\?\\UNC\\") {
        format!("\\\\{unc}")
    } else {
        text.strip_prefix("\\\\?\\").unwrap_or(&text).into()
    }
}

fn reveal_in_explorer(path: &std::path::Path) -> Result<(), String> {
    let explorer =
        PathBuf::from(std::env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into()))
            .join("explorer.exe");
    std::process::Command::new(explorer)
        .arg("/select,")
        .arg(display_path(path))
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Could not reveal image: {e}"))
}

#[cfg(all(windows, not(test)))]
fn primary_button_down() -> bool {
    #[link(name = "user32")]
    extern "system" {
        fn GetAsyncKeyState(key: i32) -> i16;
    }
    // Query only the primary mouse button during an active resize; never keys/text.
    unsafe { GetAsyncKeyState(0x01) < 0 }
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum RestoreAction {
    None,
    Cancel,
    Confirm,
}

pub(super) fn restore_dialog(ctx: &egui::Context, pending: &PendingLoad) -> RestoreAction {
    let mut action = RestoreAction::None;
    let available = ctx.screen_rect().size() - egui::vec2(48.0, 64.0);
    egui::Window::new("Restore VMU to MaplePad")
        .collapsible(false).resizable(true)
        .default_width(640.0_f32.min(available.x)).min_width(360.0)
        .max_width(available.x).max_height(available.y).vscroll(true)
        .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing.y = 10.0;
            ui.add_space(4.0);
            ui.label(RichText::new(format!("{}  /  VMU {}", pending.maplepad, pending.slot))
                .size(20.0).strong().color(accent(ui)));
            if let (Some(bus), Some(address)) = (pending.device.bus, pending.device.address) {
                ui.label(RichText::new(format!("Connection: bus {bus}, address {address}")).color(muted(ui)));
            }
            ui.label(format!("Image format: {}", pending.description));
            ui.separator();
            restore_path(ui, "Input image", &pending.input);
            let backup = pending.input.with_file_name(format!("vmu{}.before-load.bin", pending.slot));
            restore_path(ui, "Backup of current VMU", &backup);
            egui::Frame::none().fill(if ui.visuals().dark_mode { Color32::from_rgb(25,39,32) } else { Color32::from_rgb(223,235,210) }).inner_margin(10.0)
                .rounding(2.0).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new("Back up, restore, then verify").strong()
                        .color(if ui.visuals().dark_mode { Color32::from_rgb(145,211,164) } else { Color32::from_rgb(43,90,51) }));
                    ui.label(egui::RichText::new("The current VMU is saved to the backup path before writing. The restored data is then read back and checked."));
                });
            ui.label(RichText::new("This replaces the contents of the selected MaplePad VMU.").color(accent(ui)));
            ui.add_space(2.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add_sized([164.0, 30.0], egui::Button::new(RichText::new("Back up and restore").color(Color32::WHITE))
                    .fill(RED).stroke(Stroke::new(1.0_f32, accent(ui)))).clicked() {
                    action = RestoreAction::Confirm;
                }
                if ui.add_sized([80.0, 30.0], egui::Button::new("Cancel")).clicked() {
                    action = RestoreAction::Cancel;
                }
            });
        });
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        action = RestoreAction::Cancel;
    }
    action
}

fn restore_path(ui: &mut egui::Ui, title: &str, path: &std::path::Path) {
    ui.label(RichText::new(title).strong().color(text(ui)));
    egui::Frame::none()
        .fill(ui.visuals().extreme_bg_color)
        .stroke(Stroke::new(1.0_f32, rule_color(ui)))
        .inner_margin(8.0)
        .show(ui, |ui| {
            egui::ScrollArea::horizontal()
                .id_salt(title)
                .auto_shrink([false, true])
                .show_themed(ui, |ui| {
                    ui.add(
                        egui::Label::new(RichText::new(display_path(path)).monospace())
                            .wrap_mode(egui::TextWrapMode::Extend),
                    );
                });
        });
}

#[derive(Clone, Copy, PartialEq)]
pub enum ViewMode {
    Icons,
    List,
}

pub struct IconTextures {
    frames: Vec<TextureHandle>,
    mono: Option<TextureHandle>,
}

pub fn configure(ctx: &egui::Context) {
    crate::theme::apply(ctx, false);
    ctx.style_mut(|style| {
        style.spacing.item_spacing = egui::vec2(7.0, 5.0);
        style.spacing.button_padding = egui::vec2(6.0, 2.0);
        style.spacing.interact_size.y = 22.0;
        style
            .text_styles
            .insert(egui::TextStyle::Body, FontId::proportional(13.0));
        style
            .text_styles
            .insert(egui::TextStyle::Button, FontId::proportional(13.0));
        style
            .text_styles
            .insert(egui::TextStyle::Small, FontId::proportional(11.0));
    });
    // Use installed Windows fonts, including CJK collections, without redistributing them.
    let mut fonts = egui::FontDefinitions::default();
    let folder = PathBuf::from(std::env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into()))
        .join("Fonts");
    for (name, filename) in [
        ("Windows UI", "segoeui.ttf"),
        ("Japanese", "msgothic.ttc"),
        ("Japanese fallback", "YuGothR.ttc"),
        ("Chinese", "msyh.ttc"),
        ("Chinese extended", "simsun.ttc"),
        ("Korean", "malgun.ttf"),
        ("Symbols", "seguisym.ttf"),
        ("Unicode", "arial.ttf"),
    ] {
        if let Ok(bytes) = fs::read(folder.join(filename)) {
            fonts
                .font_data
                .insert(name.into(), egui::FontData::from_owned(bytes));
            if name == "Windows UI" {
                fonts
                    .families
                    .get_mut(&FontFamily::Proportional)
                    .unwrap()
                    .insert(0, name.into());
            } else {
                fonts
                    .families
                    .get_mut(&FontFamily::Proportional)
                    .unwrap()
                    .push(name.into());
            }
            fonts
                .families
                .get_mut(&FontFamily::Monospace)
                .unwrap()
                .push(name.into());
        }
    }
    ctx.set_fonts(fonts);
}

fn texture(ctx: &egui::Context, id: String, rgba: &[u8]) -> TextureHandle {
    ctx.load_texture(
        id,
        egui::ColorImage::from_rgba_unmultiplied([32, 32], rgba),
        egui::TextureOptions::NEAREST,
    )
}

fn frame_index(file: &vmu::VmuFile, time: f64) -> usize {
    vmu::animation_frame(file.icon_frames.len(), file.animation_speed, time)
}

fn icon<'a>(
    textures: Option<&'a IconTextures>,
    file: &vmu::VmuFile,
    time: f64,
) -> Option<&'a TextureHandle> {
    let textures = textures?;
    textures
        .frames
        .get(frame_index(file, time))
        .or(textures.mono.as_ref())
}

// Scale in physical pixels, not logical points, so 125%/150% Windows DPI is also crisp.
fn icon_size(ctx: &egui::Context, desired_points: f32) -> f32 {
    let ppp = ctx.pixels_per_point();
    (desired_points * ppp / 32.0).round().max(1.0) * 32.0 / ppp
}

fn paint_icon(
    ui: &egui::Ui,
    texture: Option<&TextureHandle>,
    rect: Rect,
    desired_points: f32,
    mono: bool,
) {
    let side = icon_size(ui.ctx(), desired_points);
    let ppp = ui.ctx().pixels_per_point();
    let min = (rect.center() - Vec2::splat(side / 2.0)) * ppp;
    let rect = Rect::from_min_size(
        Pos2::new(min.x.round() / ppp, min.y.round() / ppp),
        Vec2::splat(side),
    );
    if mono {
        ui.painter()
            .rect_filled(rect, 0.0, Color32::from_rgb(223, 222, 210));
    }
    if let Some(texture) = texture {
        ui.painter().image(
            texture.id(),
            rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );
    }
}

fn show_icon(ui: &mut egui::Ui, texture: Option<&TextureHandle>, size: f32, mono: bool) {
    let side = icon_size(ui.ctx(), size);
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(side), Sense::hover());
    paint_icon(ui, texture, rect, size, mono);
}

// Align every table cell to the same baseline, including CJK fallback fonts.
fn clipped_text(ui: &egui::Ui, rect: Rect, text: &str, color: Color32, size: f32) {
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    let font = FontId::proportional(size);
    let reference = painter.layout_no_wrap("Ag".into(), font.clone(), color);
    let galley = painter.layout_no_wrap(text.into(), font, color);
    let baseline = |g: &egui::Galley| {
        g.rows
            .first()
            .and_then(|row| row.glyphs.first())
            .map_or(0.0, |glyph| glyph.pos.y)
    };
    let y = rect.center().y - reference.size().y / 2.0 + baseline(&reference) - baseline(&galley);
    painter.galley(Pos2::new(rect.left() + 4.0, y), galley, color);
}

fn pane(ui: &mut egui::Ui, rect: Rect, id: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(rect.shrink(2.0)), |ui| {
        ui.set_clip_rect(rect.intersect(ui.clip_rect()));
        ui.push_id(id, |ui| add(ui));
    });
}

// Reserve the full bar first: content height must not collapse its centering rect.
fn toolbar(ui: &mut egui::Ui, height: f32, add: impl FnOnce(&mut egui::Ui)) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), height), Sense::hover());
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    child.set_clip_rect(ui.clip_rect());
    add(&mut child);
}

pub fn apply_rounding(ctx: &egui::Context) {
    let radius = 2.0_f32;
    ctx.style_mut(|style| {
        let visuals = &mut style.visuals;
        for widget in [
            &mut visuals.widgets.noninteractive,
            &mut visuals.widgets.inactive,
            &mut visuals.widgets.hovered,
            &mut visuals.widgets.active,
            &mut visuals.widgets.open,
        ] {
            widget.rounding = radius.into();
            widget.expansion = 0.0;
        }
        visuals.window_rounding = radius.into();
        visuals.menu_rounding = radius.into();
    });
}

fn splitter(ui: &mut egui::Ui, rect: Rect, id: &str, horizontal: bool) -> f32 {
    if !ui.is_enabled() {
        return 0.0;
    }
    let hit_rect = rect.expand(ui.style().interaction.interact_radius);
    let response = ui.interact(rect, ui.id().with(id), Sense::drag());
    let hovered = ui.input(|i| i.pointer.hover_pos()).is_some_and(|pos| {
        hit_rect.contains(pos) && ui.ctx().layer_id_at(pos) == Some(ui.layer_id())
    });
    let color = if hovered || response.dragged() {
        if ui.visuals().dark_mode {
            RED
        } else {
            accent(ui)
        }
    } else {
        rule_color(ui)
    };
    if horizontal {
        ui.painter()
            .hline(rect.x_range(), rect.center().y, Stroke::new(1.0_f32, color));
    } else {
        ui.painter()
            .vline(rect.center().x, rect.y_range(), Stroke::new(1.0_f32, color));
    }
    if hovered {
        ui.ctx().set_cursor_icon(if horizontal {
            egui::CursorIcon::ResizeVertical
        } else {
            egui::CursorIcon::ResizeHorizontal
        });
    }
    // Consume the event sequence, including fast press/move/release gestures that
    // can arrive within one repaint when the app is otherwise idle.
    let capture_id = ui.id().with((id, "capture"));
    let mut capture = ui.ctx().data_mut(|d| d.get_temp::<Pos2>(capture_id));
    let mut delta = 0.0;
    // Context accessors must never be called inside another context accessor:
    // egui holds its context lock for the duration of the input callback.
    let events = ui.input(|input| input.events.clone());
    for event in &events {
        match *event {
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                ..
            } if hit_rect.contains(pos) && ui.ctx().layer_id_at(pos) == Some(ui.layer_id()) => {
                capture = Some(pos);
            }
            egui::Event::PointerMoved(pos) => {
                if let Some(previous) = capture {
                    delta += if horizontal {
                        pos.y - previous.y
                    } else {
                        pos.x - previous.x
                    };
                    capture = Some(pos);
                }
            }
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                ..
            } => {
                if let Some(previous) = capture {
                    delta += if horizontal {
                        pos.y - previous.y
                    } else {
                        pos.x - previous.x
                    };
                }
                capture = None;
            }
            // The title bar emits PointerGone even while the button is held.
            egui::Event::WindowFocused(false) => capture = None,
            _ => {}
        }
    }
    #[cfg(all(windows, not(test)))]
    if capture.is_some() && !primary_button_down() {
        capture = None;
    }
    if capture.is_some() {
        ui.ctx().set_cursor_icon(if horizontal {
            egui::CursorIcon::ResizeVertical
        } else {
            egui::CursorIcon::ResizeHorizontal
        });
        ui.ctx().request_repaint_after(Duration::from_millis(16));
    }
    ui.ctx().data_mut(|d| {
        if let Some(pos) = capture {
            d.insert_temp(capture_id, pos);
        } else {
            d.remove::<Pos2>(capture_id);
        }
    });
    if delta != 0.0 {
        ui.ctx().request_repaint();
    }
    delta
}

#[cfg(test)]
mod splitter_tests {
    use super::*;

    fn frame(ctx: &egui::Context, events: Vec<egui::Event>, positions: &mut [f32; 3]) {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0))),
            events,
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                for (index, position) in positions.iter_mut().enumerate() {
                    let horizontal = index < 2;
                    let rect = if horizontal {
                        Rect::from_min_size(Pos2::new(10.0, *position), egui::vec2(750.0, 6.0))
                    } else {
                        Rect::from_min_size(Pos2::new(*position, 360.0), egui::vec2(6.0, 200.0))
                    };
                    *position += splitter(ui, rect, &format!("split-{index}"), horizontal);
                }
            });
        });
    }

    fn button(pos: Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }

    #[test]
    fn expanded_handle_keeps_capture_across_title_bar_and_cancels_on_focus_loss() {
        let ctx = egui::Context::default();
        let mut positions = [140.0, 320.0, 400.0];
        frame(&ctx, vec![], &mut positions);
        // Four points above the drawn handle is within egui's resize cursor halo.
        let start = Pos2::new(200.0, 136.0);
        frame(
            &ctx,
            vec![egui::Event::PointerMoved(start), button(start, true)],
            &mut positions,
        );
        frame(&ctx, vec![egui::Event::PointerGone], &mut positions);
        frame(
            &ctx,
            vec![egui::Event::PointerMoved(Pos2::new(200.0, 176.0))],
            &mut positions,
        );
        assert_eq!(
            positions[0], 180.0,
            "returning with the button held must continue resizing"
        );
        frame(
            &ctx,
            vec![egui::Event::WindowFocused(false)],
            &mut positions,
        );
        frame(
            &ctx,
            vec![egui::Event::PointerMoved(Pos2::new(200.0, 216.0))],
            &mut positions,
        );
        assert_eq!(positions[0], 180.0, "losing window focus cancels the grab");
    }

    #[test]
    fn dividers_drag_and_release_without_context_lock_recursion() {
        let ctx = egui::Context::default();
        let mut positions = [140.0, 320.0, 400.0];
        frame(&ctx, vec![], &mut positions);
        for (index, start, end) in [
            (0, Pos2::new(200.0, 143.0), Pos2::new(200.0, 183.0)),
            (1, Pos2::new(200.0, 323.0), Pos2::new(200.0, 263.0)),
            (2, Pos2::new(403.0, 450.0), Pos2::new(453.0, 450.0)),
        ] {
            let before = positions[index];
            frame(
                &ctx,
                vec![egui::Event::PointerMoved(start), button(start, true)],
                &mut positions,
            );
            frame(&ctx, vec![egui::Event::PointerMoved(end)], &mut positions);
            frame(&ctx, vec![button(end, false)], &mut positions);
            let delta = if index < 2 {
                end.y - start.y
            } else {
                end.x - start.x
            };
            assert_eq!(positions[index], before + delta);
            frame(
                &ctx,
                vec![egui::Event::PointerMoved(Pos2::new(20.0, 20.0))],
                &mut positions,
            );
            assert_eq!(
                positions[index],
                before + delta,
                "release must end the drag"
            );
        }
        // Even a complete gesture received between two repaints must be applied once.
        frame(
            &ctx,
            vec![
                button(Pos2::new(200.0, 183.0), true),
                egui::Event::PointerMoved(Pos2::new(200.0, 203.0)),
                button(Pos2::new(200.0, 203.0), false),
            ],
            &mut positions,
        );
        assert_eq!(positions, [200.0, 260.0, 450.0]);
    }
}

impl ManagerApp {
    pub(super) fn rebuild_icons(&mut self, ctx: &egui::Context) {
        self.icons.clear();
        self.card_icons.clear();
        let sources = self
            .devices
            .iter()
            .enumerate()
            .flat_map(|(device, d)| {
                d.slots.iter().enumerate().filter_map(move |(slot, s)| {
                    s.image.as_ref().map(|image| (device, slot, image))
                })
            })
            .chain(
                self.previews
                    .iter()
                    .enumerate()
                    .map(|(slot, (_, image))| (usize::MAX, slot, image)),
            );
        for (device, slot, image) in sources {
            if let Some(rgba) = &image.card_icon_rgba {
                self.card_icons.insert(
                    (device, slot),
                    texture(ctx, format!("card-{device}-{slot}"), rgba),
                );
            }
            for (index, file) in image.files.iter().enumerate() {
                let frames = file
                    .icon_frames
                    .iter()
                    .enumerate()
                    .map(|(frame, rgba)| {
                        texture(ctx, format!("icon-{device}-{slot}-{index}-{frame}"), rgba)
                    })
                    .collect();
                let mono = file
                    .mono_icon
                    .as_ref()
                    .map(|rgba| texture(ctx, format!("mono-{device}-{slot}-{index}"), rgba));
                self.icons
                    .insert((device, slot, index), IconTextures { frames, mono });
            }
        }
        self.hex_selection = None;
        self.hex_anchor = None;
        self.hex_hover_field = None;
        self.hex_hover_byte = None;
    }

    pub(super) fn menu(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("menu")
            .show_separator_line(false)
            .frame(
                egui::Frame::none()
                    .inner_margin(egui::Margin::symmetric(8.0, 0.0)),
            )
            .show(ctx, |ui| {
                if self.root_editor.is_some() || self.dialog_open || self.pending_load.is_some() || self.error_popup.is_some() {
                    ui.disable();
                }
                ui.spacing_mut().item_spacing.y = 0.0;
                toolbar(ui, 24.0, |ui| {
                    egui::menu::bar(ui, |ui| {
                        ui.spacing_mut().button_padding.x = 6.0;
                        ui.spacing_mut().item_spacing.x = 0.0;
                        ui.visuals_mut().widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
                        ui.visuals_mut().widgets.inactive.bg_fill = Color32::TRANSPARENT;
                        menu_button(ui, "File", |ui| {
                            if ui.add_enabled(!self.busy, egui::Button::new("Create New VMU…")).clicked() {
                                ui.close_menu();
                                self.create_new_vmu(ctx);
                            }
                            if ui
                                .add_enabled(!self.busy, egui::Button::new("Open VMU image…"))
                                .clicked()
                            {
                                ui.close_menu();
                                self.open_image(ctx);
                            }
                            if ui
                                .add_enabled(
                                    !self.busy && self.preview_active && !self.previews.is_empty(),
                                    egui::Button::new("Close local image"),
                                )
                                .clicked()
                            {
                                self.close_local(self.selected_slot, ctx);
                                ui.close_menu();
                            }
                            if ui
                                .add_enabled(
                                    !self.busy && !self.previews.is_empty(),
                                    egui::Button::new("Close all local images"),
                                )
                                .clicked()
                            {
                                self.close_all_local(ctx);
                                ui.close_menu();
                            }
                            ui.separator();
                            let dirty = self.is_dirty() && !self.busy;
                            if ui
                                .add_enabled(dirty, egui::Button::new("Save Changes"))
                                .clicked()
                            {
                                self.file_action(FileAction::Save, ctx);
                                ui.close_menu();
                            }
                            if ui
                                .add_enabled(dirty, egui::Button::new("Discard Changes"))
                                .clicked()
                            {
                                self.file_action(FileAction::Discard, ctx);
                                ui.close_menu();
                            }
                            ui.separator();
                            if ui
                                .add_enabled(!self.busy, egui::Button::new("Refresh devices"))
                                .clicked()
                            {
                                ui.close_menu();
                                self.start_scan(ctx);
                            }
                            let can_transfer = !self.busy
                                && self.current_device().is_some()
                                && self.current_image().is_some();
                            if ui
                                .add_enabled(can_transfer, egui::Button::new("Dump selected VMU…"))
                                .clicked()
                            {
                                ui.close_menu();
                                self.start_dump(ctx);
                            }
                            if ui
                                .add_enabled(
                                    !self.busy && self.current_device().is_some(),
                                    egui::Button::new("Dump all VMUs…"),
                                )
                                .clicked()
                            {
                                ui.close_menu();
                                self.start_dump_all(ctx);
                            }
                            if ui
                                .add_enabled(
                                    can_transfer,
                                    egui::Button::new("Load into selected VMU…"),
                                )
                                .clicked()
                            {
                                ui.close_menu();
                                self.choose_load(ctx);
                            }
                            ui.separator();
                            if ui.button("Exit").clicked() {
                                if self.any_dirty() || self.busy {
                                    self.error_popup = Some("Save or discard pending changes and wait for any transfer to finish before closing.".into());
                                    ui.close_menu();
                                } else { ctx.send_viewport_cmd(egui::ViewportCommand::Close); }
                            }
                        });
                        menu_button(ui, "Edit", |ui| {
                            menu_button(ui, "Music", |ui| {
                                ui.spacing_mut().button_padding.x = 10.0;
                                if let Some(music) = &mut self.music {
                                    for (enabled, label) in [(true, "On"), (false, "Off")] {
                                        if ui.selectable_label(music.enabled == enabled, label).clicked() {
                                            music.set_enabled(enabled);
                                            ui.close_menu();
                                        }
                                    }
                                    if let Some(error) = &music.error { ui.label(error); }
                                }
                            });
                            menu_button(ui, "Theme", |ui| {
                                ui.spacing_mut().button_padding.x = 10.0;
                                for (light, label) in [(true, "Light"), (false, "Dark")] {
                                    if ui.selectable_label(self.light_theme == light, label).clicked() {
                                        self.light_theme = light;
                                        crate::theme::apply(ctx, light);
                                        if let Err(error) = crate::theme::save(light) { self.error_popup = Some(error); }
                                        ui.close_menu();
                                    }
                                }
                            });
                            ui.separator();
                            if ui
                                .add_enabled(
                                    self.hex_selection.is_some(),
                                    egui::Button::new("Copy selected hex bytes"),
                                )
                                .clicked()
                            {
                                self.copy_hex(ctx);
                                ui.close_menu();
                            }
                        });
                        menu_button(ui, "Help", |ui| {
                            ui.label("Connect MaplePad in BOOTSEL mode, then Refresh.");
                            ui.label("Drag the horizontal dividers to resize the panes.");
                            ui.label("Hover a header field to highlight its bytes in Hex.");
                            ui.label("Drag across bytes to select; Ctrl+C copies hex.");
                            ui.label("← / →: VMUs · ↑ / ↓: saves");
                            ui.separator();
                            ui.label(concat!("MaplePad VMU Manager ", env!("CARGO_PKG_VERSION")));
                        });
                    });
                });
                toolbar(ui, 30.0, |ui| {
                    if !self.devices.is_empty() || !self.previews.is_empty() {
                        ui.label("Source");
                        let selected = if self.preview_active {
                            "Local VMU images".into()
                        } else {
                            self.current_device()
                                .map(|d| {
                                    format!("MaplePad {} · {}", d.firmware.label(), d.id.label())
                                })
                                .unwrap_or_default()
                        };
                        ui.add_enabled_ui(!self.busy && self.pending_load.is_none(), |ui| {
                            egui::ComboBox::from_id_salt("device")
                                .selected_text(selected)
                                .width(205.0)
                                .truncate()
                                .show_ui(ui, |ui| {
                                    mark_menu_open(ui.ctx());
                                    if !self.previews.is_empty()
                                        && ui
                                            .selectable_label(
                                                self.preview_active,
                                                "Local VMU images",
                                            )
                                            .clicked()
                                    {
                                        self.preview_active = true;
                                        self.selected_slot = 0;
                                        self.select_file(None);
                                    }
                                    let mut clicked = None;
                                    for (index, device) in self.devices.iter().enumerate() {
                                        if ui
                                            .selectable_label(
                                                !self.preview_active
                                                    && self.selected_device == index,
                                                format!(
                                                    "MaplePad {} · {}",
                                                    device.firmware.label(),
                                                    device.id.label()
                                                ),
                                            )
                                            .clicked()
                                        {
                                            clicked = Some(index);
                                        }
                                    }
                                    if let Some(index) = clicked {
                                        self.preview_active = false;
                                        self.selected_device = index;
                                        self.selected_slot = 0;
                                        self.select_file(None);
                                    }
                                });
                        });
                    }
                    if ui
                        .add_enabled(
                            !self.busy && self.pending_load.is_none(),
                            egui::Button::new("Refresh"),
                        )
                        .on_hover_text("Connect MaplePad in BOOTSEL mode, then refresh devices.")
                        .clicked()
                    {
                        self.start_scan(ctx);
                    }
                    if self.busy {
                        ui.spinner();
                    }
                    ui.add(
                        egui::Label::new(RichText::new(&self.status).small().color(text(ui)))
                            .truncate(),
                    )
                    .on_hover_text(&self.status);
                });
            });
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::C)) {
            self.copy_hex(ctx);
        }
    }

    pub(super) fn select_file(&mut self, index: Option<usize>) {
        self.selected_file = index;
        self.hex_selection = None;
        self.hex_anchor = None;
        self.hex_hover_field = None;
        self.hex_hover_byte = None;
        self.hex_scroll_to = Some(0);
    }

    fn copy_hex(&self, ctx: &egui::Context) {
        if let (Some(image), Some((start, end))) = (self.current_image(), self.hex_selection) {
            let bytes = self
                .selected_file
                .and_then(|i| image.files.get(i))
                .map(|f| f.bytes.as_slice())
                .unwrap_or(&image.bytes);
            if let Some(bytes) = bytes.get(start..end.min(bytes.len())) {
                ctx.copy_text(
                    bytes
                        .iter()
                        .map(|b| format!("{b:02X}"))
                        .collect::<Vec<_>>()
                        .join(" "),
                );
            }
        }
    }

    pub(super) fn workspace(&mut self, ui: &mut egui::Ui) {
        if self.root_editor.is_some()
            || self.dialog_open
            || self.pending_load.is_some()
            || self.error_popup.is_some()
        {
            ui.disable();
        }
        let rect = ui.available_rect_before_wrap();
        let top_max = (rect.height() - 225.0).max(126.0);
        self.top_height = self.top_height.clamp(126.0, top_max);
        let top_bar = Rect::from_min_size(
            Pos2::new(rect.left(), rect.top() + self.top_height),
            egui::vec2(rect.width(), 6.0),
        );
        let body_start = rect.top() + self.top_height + 6.0;
        let body_height = (rect.bottom() - body_start).max(0.0);
        self.middle_height = self
            .middle_height
            .clamp(85.0, (body_height - 130.0).max(85.0));
        let middle_bar = Rect::from_min_size(
            Pos2::new(rect.left(), body_start + self.middle_height),
            egui::vec2(rect.width(), 6.0),
        );
        let top = Rect::from_min_max(
            rect.min,
            Pos2::new(rect.right(), rect.top() + self.top_height),
        );
        let middle = Rect::from_min_max(
            Pos2::new(rect.left(), body_start),
            Pos2::new(rect.right(), body_start + self.middle_height),
        );
        let bottom = Rect::from_min_max(Pos2::new(rect.left(), middle.bottom() + 6.0), rect.max);
        pane(ui, top.shrink2(egui::vec2(8.0, 4.0)), "cards", |ui| {
            self.slot_cards(ui)
        });
        pane(
            ui,
            Rect::from_min_max(
                Pos2::new(middle.left() + 8.0, middle.top() - 4.0),
                Pos2::new(middle.right() - 8.0, middle.bottom()),
            ),
            "files",
            |ui| self.file_browser(ui),
        );
        pane(ui, bottom.shrink2(egui::vec2(8.0, 0.0)), "detail", |ui| {
            self.details(ui)
        });
        // Register handles last so adjacent scroll areas cannot capture their drag.
        self.top_height =
            (self.top_height + splitter(ui, top_bar, "top-split", true)).clamp(126.0, top_max);
        self.middle_height = (self.middle_height + splitter(ui, middle_bar, "middle-split", true))
            .clamp(85.0, (body_height - 130.0).max(85.0));
        let time = self.animation_started.elapsed().as_secs_f64();
        if let Some(delay) = self
            .current_image()
            .into_iter()
            .flat_map(|i| &i.files)
            .filter(|f| f.icon_frames.len() > 1)
            .map(|f| {
                let duration = vmu::animation_seconds(f.animation_speed);
                duration - time.rem_euclid(duration)
            })
            .reduce(f64::min)
        {
            ui.ctx()
                .request_repaint_after(Duration::from_secs_f64(delay.max(0.001)));
        }
    }

    fn slot_cards(&mut self, ui: &mut egui::Ui) {
        // The source selector owns the entire strip: local and device cards never mix.
        let mut cards = Vec::new();
        if let Some(device) = self.current_device() {
            for (slot, state) in device.slots.iter().enumerate() {
                if let Some(image) = &state.image {
                    cards.push((
                        false,
                        slot,
                        format!(
                            "VMU {slot}{}",
                            if image.original_bytes.is_some() {
                                " *"
                            } else {
                                ""
                            }
                        ),
                        None,
                        image.capacity,
                        image.free,
                        image.card_color,
                    ));
                }
            }
        }
        if self.preview_active {
            for (slot, (path, image)) in self.previews.iter().enumerate() {
                cards.push((
                    true,
                    slot,
                    format!(
                        "{}{}",
                        if image.original_bytes.is_some() {
                            "* "
                        } else {
                            ""
                        },
                        path.file_name().unwrap_or_default().to_string_lossy()
                    ),
                    Some(path.clone()),
                    image.capacity,
                    image.free,
                    image.card_color,
                ));
            }
        }
        if cards.is_empty() {
            ui.painter().text(
                ui.available_rect_before_wrap().center(),
                Align2::CENTER_CENTER,
                EMPTY_MESSAGE,
                FontId::proportional(13.0),
                muted(ui),
            );
            return;
        }
        let scroll_to_card = std::mem::take(&mut self.card_scroll);
        let enabled = self.pending_load.is_none();
        let mut select = None;
        let mut action = None;
        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show_themed(ui, |ui| {
                ui.horizontal(|ui| {
                    for (local, slot, title, path, capacity, free, color) in cards {
                        // Reserve space for the glow inside the scroll area's clip bounds.
                        let (outer, response) =
                            ui.allocate_exact_size(egui::vec2(114.0, 126.0), Sense::click());
                        let rect = outer.shrink(5.0);
                        let selected = self.preview_active == local && self.selected_slot == slot;
                        if selected && scroll_to_card {
                            response.scroll_to_me(None);
                        }
                        let [r, g, b, a] = color.unwrap_or([105, 110, 120, 255]);
                        // Preserve the root RGB exactly; only alpha controls intensity.
                        let tint = |opacity: f32| {
                            Color32::from_rgba_unmultiplied(
                                r,
                                g,
                                b,
                                (f32::from(a) * opacity).round() as u8,
                            )
                        };
                        if selected {
                            // One continuous, softly blurred halo instead of nested
                            // translucent rectangles that create pale stepped borders.
                            let opacity = (f32::from(a) * 0.35).round() as u8;
                            let shade = if ui.visuals().dark_mode { 1.0 } else { 0.80 };
                            let premultiply = |c: u8| {
                                (f32::from(c) * shade * f32::from(opacity) / 255.0).round() as u8
                            };
                            ui.painter().add(
                                egui::epaint::Shadow {
                                    offset: Vec2::ZERO,
                                    blur: 8.0,
                                    spread: 0.0,
                                    color: Color32::from_rgba_premultiplied(
                                        premultiply(r),
                                        premultiply(g),
                                        premultiply(b),
                                        opacity,
                                    ),
                                }
                                .as_shape(rect, 2.0_f32),
                            );
                            // Root RGB/alpha form a translucent wash over the current
                            // theme, with no opaque charcoal underlay or edge stroke.
                            ui.painter().rect_filled(
                                rect,
                                2.0_f32,
                                crate::theme::card_fill(
                                    [r, g, b, a],
                                    ui.visuals().panel_fill,
                                    !ui.visuals().dark_mode,
                                ),
                            );
                        } else {
                            let neutral = if ui.visuals().dark_mode {
                                Color32::from_rgba_unmultiplied(145, 145, 155, 18)
                            } else {
                                Color32::from_rgba_unmultiplied(95, 75, 65, 16)
                            };
                            ui.painter().rect_filled(rect, 2.0_f32, neutral);
                        }
                        let label_color = text(ui);
                        let icon_rect = Rect::from_min_max(
                            rect.min + egui::vec2(4.0, 3.0),
                            Pos2::new(rect.right() - 4.0, rect.top() + 75.0),
                        );
                        let device = if local {
                            usize::MAX
                        } else {
                            self.selected_device
                        };
                        paint_icon(
                            ui,
                            self.card_icons.get(&(device, slot)),
                            icon_rect,
                            64.0,
                            false,
                        );
                        ui.painter().hline(
                            rect.left() + 3.0..=rect.right() - 3.0,
                            rect.top() + 78.0,
                            Stroke::new(
                                1.0_f32,
                                if selected { tint(0.16) } else { rule_color(ui) },
                            ),
                        );
                        clipped_text(
                            ui,
                            Rect::from_min_size(
                                rect.min + egui::vec2(2.0, 80.0),
                                egui::vec2(100.0, 17.0),
                            ),
                            &title,
                            label_color,
                            12.0,
                        );
                        clipped_text(
                            ui,
                            Rect::from_min_size(
                                rect.min + egui::vec2(2.0, 97.0),
                                egui::vec2(100.0, 16.0),
                            ),
                            &format!("{} / {capacity} blocks", capacity - free),
                            label_color,
                            10.5,
                        );
                        if enabled && response.clicked() {
                            select = Some((local, slot));
                        }
                        let response = if let Some(path) = &path {
                            response.on_hover_text(display_path(path))
                        } else {
                            response
                        };
                        if enabled {
                            response.context_menu(|ui| {
                                scroll_menu(ui, |ui| {
                                    if ui
                                        .add_enabled(
                                            !self.busy,
                                            egui::Button::new("Edit Root Block…"),
                                        )
                                        .clicked()
                                    {
                                        action = Some(CardAction::Root(local, slot));
                                        ui.close_menu();
                                    }
                                    if let Some(path) = &path {
                                        if ui
                                            .add_enabled(
                                                !self.busy,
                                                egui::Button::new("Save Copy As…"),
                                            )
                                            .clicked()
                                        {
                                            action = Some(CardAction::SaveCopy(slot));
                                            ui.close_menu();
                                        }
                                        if ui.button("Reveal in Explorer").clicked() {
                                            action = Some(CardAction::Reveal(path.clone()));
                                            ui.close_menu();
                                        }
                                        if ui
                                            .add_enabled(
                                                !self.busy,
                                                egui::Button::new("Close image"),
                                            )
                                            .clicked()
                                        {
                                            action = Some(CardAction::Close(slot));
                                            ui.close_menu();
                                        }
                                    } else {
                                        if ui
                                            .add_enabled(
                                                !self.busy,
                                                egui::Button::new("Dump VMU..."),
                                            )
                                            .clicked()
                                        {
                                            action = Some(CardAction::Dump(slot));
                                            ui.close_menu();
                                        }
                                        if ui
                                            .add_enabled(
                                                !self.busy,
                                                egui::Button::new("Load VMU..."),
                                            )
                                            .clicked()
                                        {
                                            action = Some(CardAction::Load(slot));
                                            ui.close_menu();
                                        }
                                    }
                                });
                            });
                        }
                    }
                });
            });
        if let Some((local, slot)) = select {
            self.preview_active = local;
            self.selected_slot = slot;
            self.select_file(None);
        }
        match action {
            Some(CardAction::SaveCopy(slot)) => self.save_copy_as(slot, ui.ctx()),
            Some(CardAction::Root(local, slot)) => {
                self.preview_active = local;
                self.selected_slot = slot;
                self.select_file(None);
                self.open_root_editor();
            }
            Some(CardAction::Reveal(path)) => {
                if let Err(error) = reveal_in_explorer(&path) {
                    self.status = error;
                }
            }
            Some(CardAction::Close(slot)) => self.close_local(slot, ui.ctx()),
            Some(CardAction::Dump(slot)) => {
                self.preview_active = false;
                self.selected_slot = slot;
                self.select_file(None);
                self.start_dump(ui.ctx());
            }
            Some(CardAction::Load(slot)) => {
                self.preview_active = false;
                self.selected_slot = slot;
                self.select_file(None);
                self.choose_load(ui.ctx());
            }
            None => {}
        }
    }

    fn file_browser(&mut self, ui: &mut egui::Ui) {
        let navigate = std::mem::take(&mut self.navigation_scroll);
        ui.spacing_mut().item_spacing.y = 0.0;
        let mut action = None;
        let selected = self.selected_file;
        let dirty = self.is_dirty();
        let has_clipboard = self.clipboard.is_some();
        let enabled = !self.busy && self.current_image().is_some();
        let moves = if self.sort_column == sorting::Column::FirstBlock
            && self.view_mode == ViewMode::List
        {
            selected.and_then(|index| {
                self.current_image().map(|image| {
                    (
                        vmu::editing::can_move(image, index, self.sort_descending),
                        vmu::editing::can_move(image, index, !self.sort_descending),
                        self.sort_descending,
                    )
                })
            })
        } else {
            None
        };
        ui.add_space(3.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(5.0, 4.0);
            ui.set_min_height(24.0);
            ui.label("View:");
            ui.radio_value(&mut self.view_mode, ViewMode::Icons, "Icons");
            ui.radio_value(&mut self.view_mode, ViewMode::List, "List");
            if let Some(image) = self.current_image() {
                ui.separator();
                ui.label(format!("{} files", image.files.len()));
                ui.separator();
                ui.label("Block");
                let used = image.capacity - image.free;
                let (bar, _) = ui.allocate_exact_size(egui::vec2(130.0, 16.0), Sense::hover());
                let fraction = (used as f32 / image.capacity.max(1) as f32).clamp(0.0, 1.0);
                let filled =
                    Rect::from_min_size(bar.min, egui::vec2(bar.width() * fraction, bar.height()));
                ui.painter()
                    .rect_filled(bar, 0.0, ui.visuals().extreme_bg_color);
                ui.painter().rect_filled(
                    filled,
                    0.0,
                    if ui.visuals().dark_mode {
                        RED
                    } else {
                        accent(ui)
                    },
                );
                ui.painter()
                    .rect_stroke(bar, 0.0, Stroke::new(1.0_f32, rule_color(ui)));
                let label = format!("{used}/{} ({:.0}%)", image.capacity, fraction * 100.0);
                let unfilled = Rect::from_min_max(filled.right_top(), bar.right_bottom());
                // Keep one centered label, with contrast matched to the surface
                // beneath each portion, including when the fill crosses a glyph.
                for (region, color) in [
                    (filled, Color32::WHITE),
                    (
                        unfilled,
                        if ui.visuals().dark_mode {
                            Color32::WHITE
                        } else {
                            text(ui)
                        },
                    ),
                ] {
                    ui.painter()
                        .with_clip_rect(ui.clip_rect().intersect(region))
                        .text(
                            bar.center(),
                            Align2::CENTER_CENTER,
                            &label,
                            FontId::proportional(11.0),
                            color,
                        );
                }
                ui.label(format!("Free: {}", image.free));
            }
            file_action_buttons(
                ui,
                selected,
                dirty,
                has_clipboard,
                enabled,
                moves,
                &mut action,
            );
        });
        ui.add_space(3.0);
        let (line, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), Sense::hover());
        ui.painter().hline(
            line.x_range(),
            line.center().y,
            Stroke::new(1.0_f32, rule_color(ui)),
        );
        let Some(image) = self.current_image() else {
            let message = self
                .current_device()
                .and_then(|d| d.slots.get(self.selected_slot))
                .and_then(|s| s.error.as_deref())
                .unwrap_or(EMPTY_MESSAGE);
            ui.centered_and_justified(|ui| {
                ui.label(RichText::new(message).color(muted(ui)));
            });
            return;
        };
        let body = ui.available_rect_before_wrap();
        let background = ui.interact(body, ui.id().with("paste-background"), Sense::click());
        save_context(&background, None, has_clipboard, enabled, &mut action);
        let order = sorting::indices(image, self.sort_column, self.sort_descending);
        let mut clicked_sort = None;
        let time = self.animation_started.elapsed().as_secs_f64();
        let mut clicked = None;
        let device = self.icon_device_index();
        let slot = self.selected_slot;
        match self.view_mode {
            ViewMode::Icons => {
                ui.spacing_mut().item_spacing.y = 4.0;
                ui.add_space(4.0);
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show_themed(ui, |ui| {
                        let cols = (ui.available_width() / 92.0).floor().max(1.0) as usize;
                        for files in order.chunks(cols) {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 4.0;
                                for &index in files {
                                    let file = &image.files[index];
                                    let (rect, response) = ui.allocate_exact_size(
                                        egui::vec2(88.0, 96.0),
                                        Sense::click(),
                                    );
                                    if navigate && self.selected_file == Some(index) {
                                        response.scroll_to_me(None);
                                    }
                                    if self.selected_file == Some(index) {
                                        ui.painter().rect_filled(rect, 2.0_f32, selection_fill(ui));
                                    } else if response.hovered() {
                                        ui.painter().rect_filled(
                                            rect,
                                            2.0_f32,
                                            ui.visuals().widgets.hovered.weak_bg_fill,
                                        );
                                    }
                                    let icon_rect =
                                        Rect::from_min_size(rect.min, egui::vec2(88.0, 76.0));
                                    paint_icon(
                                        ui,
                                        icon(self.icons.get(&(device, slot, index)), file, time),
                                        icon_rect,
                                        64.0,
                                        file.icon_frames.is_empty() && file.mono_icon.is_some(),
                                    );
                                    clipped_text(
                                        ui,
                                        Rect::from_min_size(
                                            rect.min + egui::vec2(0.0, 76.0),
                                            egui::vec2(88.0, 19.0),
                                        ),
                                        &file.name,
                                        selected_text(self.selected_file == Some(index), ui),
                                        10.5,
                                    );
                                    save_context(
                                        &response,
                                        Some(index),
                                        has_clipboard,
                                        enabled,
                                        &mut action,
                                    );
                                    if response.secondary_clicked() {
                                        clicked = Some(index);
                                    }
                                    if response
                                        .on_hover_text(format!(
                                            "{}\n{}\n{}",
                                            file.name, file.vm_description, file.dc_description
                                        ))
                                        .clicked()
                                    {
                                        clicked = Some(index);
                                    }
                                }
                            });
                        }
                        let empty_height = (4.0 * 100.0_f32).max(ui.available_height());
                        let (_, response) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), empty_height),
                            Sense::click(),
                        );
                        save_context(&response, None, has_clipboard, enabled, &mut action);
                    });
            }
            ViewMode::List => {
                egui::ScrollArea::horizontal()
                    .id_salt("file-columns")
                    .auto_shrink([false, false])
                    .show_themed(ui, |ui| {
                        let widths = sorting::COLUMNS.map(|(_, _, width)| width);
                        let width = widths.iter().sum::<f32>().max(ui.available_width());
                        let (head, _) =
                            ui.allocate_exact_size(egui::vec2(width, 25.0), Sense::hover());
                        let mut x = head.left();
                        for (column, title, col_width) in sorting::COLUMNS {
                            let cell = Rect::from_min_size(
                                Pos2::new(x, head.top()),
                                egui::vec2(col_width, head.height()),
                            );
                            let active = self.sort_column == column;
                            if active {
                                ui.painter().rect_filled(cell, 2.0, selection_fill(ui));
                            }
                            let label = if active {
                                format!("{title} {}", if self.sort_descending { "↓" } else { "↑" })
                            } else {
                                title.into()
                            };
                            clipped_text(ui, cell, &label, selected_text(active, ui), 13.0);
                            if ui
                                .interact(
                                    cell,
                                    ui.id().with(("sort", column as usize)),
                                    Sense::click(),
                                )
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .clicked()
                            {
                                clicked_sort = Some(column);
                            }
                            x += col_width;
                        }
                        ui.painter().hline(
                            head.x_range(),
                            head.bottom(),
                            Stroke::new(1.0_f32, rule_color(ui)),
                        );
                        let row_height = icon_size(ui.ctx(), 32.0).max(32.0) + 4.0;
                        ui.spacing_mut().item_spacing.y = 0.0;
                        egui::ScrollArea::vertical()
                            .id_salt("file-rows")
                            .auto_shrink([false, false])
                            .show_rows_themed(ui, row_height, image.files.len() + 4, |ui, rows| {
                                ui.spacing_mut().item_spacing.y = 0.0;
                                if navigate {
                                    if let Some(row) =
                                        order.iter().position(|&i| Some(i) == self.selected_file)
                                    {
                                        let y = ui.cursor().top()
                                            + (row as f32 - rows.start as f32) * row_height;
                                        ui.scroll_to_rect(
                                            Rect::from_min_size(
                                                Pos2::new(ui.cursor().left(), y),
                                                egui::vec2(width, row_height),
                                            ),
                                            None,
                                        );
                                    }
                                }
                                for row in rows {
                                    let (rect, response) = ui.allocate_exact_size(
                                        egui::vec2(width, row_height),
                                        Sense::click(),
                                    );
                                    let Some(&index) = order.get(row) else {
                                        save_context(
                                            &response,
                                            None,
                                            has_clipboard,
                                            enabled,
                                            &mut action,
                                        );
                                        continue;
                                    };
                                    let file = &image.files[index];
                                    save_context(
                                        &response,
                                        Some(index),
                                        has_clipboard,
                                        enabled,
                                        &mut action,
                                    );
                                    if response.secondary_clicked() {
                                        clicked = Some(index);
                                    }
                                    let fill = if self.selected_file == Some(index) {
                                        selection_fill(ui)
                                    } else if row % 2 == 0 {
                                        stripe(ui)
                                    } else {
                                        Color32::TRANSPARENT
                                    };
                                    ui.painter().rect_filled(rect, 0.0, fill);
                                    paint_icon(
                                        ui,
                                        icon(self.icons.get(&(device, slot, index)), file, time),
                                        Rect::from_min_size(rect.min, egui::vec2(36.0, row_height)),
                                        32.0,
                                        file.icon_frames.is_empty() && file.mono_icon.is_some(),
                                    );
                                    let values = [
                                        file.name.clone(),
                                        file.vm_description.clone(),
                                        file.dc_description.clone(),
                                        file.blocks.to_string(),
                                        file.kind.into(),
                                        if file.copy_protected {
                                            "Yes".into()
                                        } else {
                                            String::new()
                                        },
                                        file.created.clone(),
                                        file.first_block.to_string(),
                                        file.crc
                                            .map(|c| format!("0x{c:04X}"))
                                            .unwrap_or_else(|| "—".into()),
                                    ];
                                    let mut x = rect.left();
                                    for (column, (value, col_width)) in
                                        values.iter().zip(widths).enumerate()
                                    {
                                        let indent = if column == 0 { 36.0 } else { 0.0 };
                                        let cell = Rect::from_min_size(
                                            Pos2::new(x + indent, rect.top()),
                                            egui::vec2(col_width - indent, row_height),
                                        );
                                        clipped_text(
                                            ui,
                                            cell,
                                            value,
                                            if column == 8 && file.crc_valid == Some(false) {
                                                if ui.visuals().dark_mode {
                                                    Color32::from_rgb(255, 105, 91)
                                                } else {
                                                    RED
                                                }
                                            } else {
                                                selected_text(self.selected_file == Some(index), ui)
                                            },
                                            13.0,
                                        );
                                        x += col_width;
                                        ui.painter().vline(
                                            x,
                                            rect.y_range(),
                                            Stroke::new(1.0_f32, rule_color(ui)),
                                        );
                                    }
                                    if response
                                        .on_hover_text(format!(
                                            "{}\n{}\n{}",
                                            file.name, file.vm_description, file.dc_description
                                        ))
                                        .clicked()
                                    {
                                        clicked = Some(index);
                                    }
                                }
                            });
                    });
            }
        }
        if let Some(column) = clicked_sort {
            if self.sort_column == column {
                self.sort_descending = !self.sort_descending;
            } else {
                self.sort_column = column;
                self.sort_descending = false;
            }
        }
        if let Some(index) = clicked {
            self.select_file(Some(index));
        }
        if let Some(action) = action {
            self.file_action(action, ui.ctx());
        }
    }

    fn details(&mut self, ui: &mut egui::Ui) {
        toolbar(ui, 30.0, |ui| {
            for (tab, label) in [(DetailTab::Detail, "Detail"), (DetailTab::Hex, "Hex")] {
                if ui
                    .add_sized(
                        [52.0, 24.0],
                        egui::Button::new(
                            RichText::new(label).color(selected_text(self.detail_tab == tab, ui)),
                        )
                        .selected(self.detail_tab == tab)
                        .rounding(2.0_f32),
                    )
                    .clicked()
                {
                    self.detail_tab = tab;
                }
            }
            if let Some(file) = self
                .current_image()
                .and_then(|i| self.selected_file.and_then(|f| i.files.get(f)))
            {
                ui.label(RichText::new(&file.name).color(muted(ui)));
            }
        });
        ui.separator();
        let Some(image) = self.current_image() else {
            return;
        };
        let Some(file) = self.selected_file.and_then(|i| image.files.get(i)).cloned() else {
            if self.detail_tab == DetailTab::Hex {
                let bytes = image.bytes.clone();
                self.hex_bytes(ui, &bytes);
            } else {
                egui::ScrollArea::both().show_themed(ui, |ui| {
                    ui.label(format!(
                        "{} · {} used / {} blocks · {} files",
                        image.format.label(),
                        image.capacity - image.free,
                        image.capacity,
                        image.files.len()
                    ));
                    ui.label(
                        RichText::new("Select a file to inspect its metadata, icons, and header.")
                            .color(muted(ui)),
                    );
                });
            }
            return;
        };
        let rect = ui.available_rect_before_wrap();
        let width = if self.detail_tab == DetailTab::Detail {
            &mut self.detail_width
        } else {
            &mut self.hex_width
        };
        *width = (*width).clamp(250.0, (rect.width() - 230.0).max(250.0));
        let handle = Rect::from_min_size(
            Pos2::new(rect.left() + *width, rect.top()),
            egui::vec2(6.0, rect.height()),
        );
        let left = Rect::from_min_max(rect.min, Pos2::new(rect.left() + *width, rect.bottom()));
        let right = Rect::from_min_max(Pos2::new(left.right() + 10.0, rect.top()), rect.max);
        match self.detail_tab {
            DetailTab::Detail => {
                pane(ui, left, "metadata", |ui| self.metadata(ui, &file));
                pane(ui, right, "icons", |ui| self.icon_details(ui, &file));
            }
            DetailTab::Hex => {
                pane(ui, right, "header", |ui| self.header_fields(ui, &file));
                pane(ui, left, "hex", |ui| self.hex_bytes(ui, &file.bytes));
            }
        }
        let delta = splitter(ui, handle, "detail-columns", false);
        let width = if self.detail_tab == DetailTab::Detail {
            &mut self.detail_width
        } else {
            &mut self.hex_width
        };
        *width = (*width + delta).clamp(250.0, (rect.width() - 230.0).max(250.0));
    }

    fn metadata(&self, ui: &mut egui::Ui, file: &vmu::VmuFile) {
        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show_themed(ui, |ui| {
                egui::Grid::new("metadata-grid")
                    .num_columns(2)
                    .min_col_width(92.0)
                    .min_row_height(18.0)
                    .spacing([10.0, 6.0])
                    .show(ui, |ui| {
                        let special = file.kind == "ICON";
                        for (key, value) in [
                            ("Name", file.name.clone()),
                            ("Type", file.kind.into()),
                            ("VM Desc", file.vm_description.clone()),
                            ("DC Desc", file.dc_description.clone()),
                            ("Application", file.application.clone()),
                            ("Blocks", file.blocks.to_string()),
                            ("1st block", file.first_block.to_string()),
                            (
                                "CRC",
                                file.crc
                                    .map(|c| {
                                        format!(
                                            "0x{c:04X}{}",
                                            if file.crc_valid == Some(false) {
                                                " · mismatch"
                                            } else {
                                                ""
                                            }
                                        )
                                    })
                                    .unwrap_or_default(),
                            ),
                            ("Created", file.created.clone()),
                            (
                                "Copy Protected",
                                if file.copy_protected {
                                    "Yes".into()
                                } else {
                                    "No".into()
                                },
                            ),
                            (
                                "Icon Count",
                                if special {
                                    String::new()
                                } else {
                                    file.icon_count.to_string()
                                },
                            ),
                            (
                                "Anim Speed",
                                if special {
                                    String::new()
                                } else {
                                    format!(
                                        "{} × 1/30 s ({:.0} ms/frame)",
                                        file.animation_speed,
                                        f64::from(file.animation_speed.max(1)) * 1000.0 / 30.0
                                    )
                                },
                            ),
                            (
                                "Eyecatch Type",
                                if special {
                                    String::new()
                                } else {
                                    vmu::eyecatch_label(file.eyecatch_type).into()
                                },
                            ),
                            (
                                "Data Size",
                                if special {
                                    String::new()
                                } else {
                                    format!("{} bytes", file.data_size)
                                },
                            ),
                            ("Header Offset", format!("0x{:X}", file.header_offset)),
                        ] {
                            ui.label(RichText::new(key).color(muted(ui)));
                            ui.add(
                                egui::Label::new(if value.is_empty() { "—" } else { &value })
                                    .wrap(),
                            );
                            ui.end_row();
                        }
                    });
            });
    }

    fn icon_details(&mut self, ui: &mut egui::Ui, file: &vmu::VmuFile) {
        let key = (
            self.icon_device_index(),
            self.selected_slot,
            self.selected_file.unwrap_or(0),
        );
        let textures = self.icons.get(&key);
        let time = self.animation_started.elapsed().as_secs_f64();
        let mut export_action = None;
        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show_themed(ui, |ui| {
                egui::Frame::none().inner_margin(2.0).show(ui, |ui| {
                    if !file.icon_frames.is_empty() {
                        ui.horizontal_top(|ui| {
                            ui.vertical(|ui| {
                                ui.label("Icon");
                                show_icon(ui, icon(textures, file, time), 96.0, false);
                            });
                            if let Some(palette) = &file.palette {
                                ui.add_space(10.0);
                                ui.vertical(|ui| {
                                    ui.label("Palette");
                                    egui::Grid::new("palette")
                                        .min_col_width(18.0)
                                        .min_row_height(18.0)
                                        .spacing([2.0, 2.0])
                                        .show(ui, |ui| {
                                            for (index, color) in palette.iter().enumerate() {
                                                let (rect, response) = ui.allocate_exact_size(
                                                    egui::vec2(18.0, 18.0),
                                                    Sense::hover(),
                                                );
                                                // Composite actual alpha over a neutral checkerboard.
                                                for y in 0..4 {
                                                    for x in 0..4 {
                                                        let tile = Rect::from_min_size(
                                                            rect.min
                                                                + egui::vec2(
                                                                    x as f32 * 4.5,
                                                                    y as f32 * 4.5,
                                                                ),
                                                            Vec2::splat(4.5),
                                                        );
                                                        ui.painter().rect_filled(
                                                            tile,
                                                            0.0,
                                                            Color32::from_gray(
                                                                if (x + y) % 2 == 0 {
                                                                    95
                                                                } else {
                                                                    175
                                                                },
                                                            ),
                                                        );
                                                    }
                                                }
                                                ui.painter().rect(
                                                    rect,
                                                    0.0,
                                                    Color32::from_rgba_unmultiplied(
                                                        color[0], color[1], color[2], color[3],
                                                    ),
                                                    Stroke::new(1.0_f32, rule_color(ui)),
                                                );
                                                response.on_hover_text(format!(
                                                    "Color {index:X}\nR {}  G {}  B {}  A {}",
                                                    color[0], color[1], color[2], color[3]
                                                ));
                                                if index % 4 == 3 {
                                                    ui.end_row();
                                                }
                                            }
                                        });
                                });
                            }
                        });
                        if file.icon_frames.len() > 1 {
                            ui.label(
                                RichText::new(format!("{} frames", file.icon_frames.len()))
                                    .small()
                                    .color(muted(ui)),
                            );
                            ui.horizontal(|ui| {
                                if let Some(textures) = textures {
                                    for frame in &textures.frames {
                                        show_icon(ui, Some(frame), 64.0, false);
                                    }
                                }
                            });
                        }
                        menu_button(ui, "Save image…", |ui| {
                            if file.icon_frames.len() == 1 {
                                if ui.button("PNG image...").clicked() {
                                    export_action = Some((false, false, 0));
                                    ui.close_menu();
                                }
                            } else {
                                if ui.button("Animation as GIF...").clicked() {
                                    export_action = Some((true, false, 0));
                                    ui.close_menu();
                                }
                                menu_button(ui, "Frame as PNG", |ui| {
                                    for frame in 0..file.icon_frames.len() {
                                        if ui.button(format!("Frame {}...", frame + 1)).clicked() {
                                            export_action = Some((false, false, frame));
                                            ui.close_menu();
                                        }
                                    }
                                });
                            }
                        });
                    }
                    if file.mono_icon.is_some() {
                        ui.add_space(8.0);
                        ui.label("Mono");
                        show_icon(ui, textures.and_then(|t| t.mono.as_ref()), 96.0, true);
                        if ui.button("Save mono PNG…").clicked() {
                            export_action = Some((false, true, 0));
                        }
                    }
                    if file.icon_frames.is_empty() && file.mono_icon.is_none() {
                        ui.label(RichText::new("No icon in this file").color(muted(ui)));
                    }
                });
            });
        if let Some((gif, mono, frame)) = export_action {
            let extension = if gif { "gif" } else { "png" };
            let name = format!(
                "{}{}.{}",
                file.name,
                if mono {
                    "-mono".into()
                } else if !gif && file.icon_frames.len() > 1 {
                    format!("-frame{}", frame + 1)
                } else {
                    String::new()
                },
                extension
            );
            let file = file.clone();
            self.run_dialog(ui.ctx(), move |dialog| {
                let Some(path) = dialog
                    .set_file_name(name)
                    .add_filter(
                        if gif { "GIF animation" } else { "PNG image" },
                        &[extension],
                    )
                    .save_file()
                else {
                    return Event::DialogDone(Ok(None));
                };
                let result = if gif {
                    export::gif(&path, &file.icon_frames, file.animation_speed)
                } else {
                    export::png(
                        &path,
                        if mono {
                            file.mono_icon.as_ref().unwrap()
                        } else {
                            &file.icon_frames[frame]
                        },
                    )
                };
                Event::DialogDone(
                    result
                        .map(|()| Some(format!("Saved {}", display_path(&path))))
                        .map_err(|error| format!("Image export failed: {error}")),
                )
            });
        }
    }

    fn header_fields(&mut self, ui: &mut egui::Ui, file: &vmu::VmuFile) {
        let mut hovered = None;
        ui.label(if file.kind == "ICON" {
            "ICONDATA_VMS"
        } else {
            "DATA header"
        });
        ui.add_space(7.0);
        egui::ScrollArea::both()
            .id_salt("header-scroll")
            .auto_shrink([false, false])
            .show_themed(ui, |ui| {
                let widths = [180.0, 95.0, 55.0, 240.0];
                let total: f32 = widths.iter().sum();
                let (rect, _) = ui.allocate_exact_size(egui::vec2(total, 25.0), Sense::hover());
                let mut x = rect.left();
                for (title, width) in ["Field", "Offset", "Size", "Value"].iter().zip(widths) {
                    clipped_text(
                        ui,
                        Rect::from_min_size(
                            Pos2::new(x, rect.top()),
                            egui::vec2(width, rect.height()),
                        ),
                        title,
                        accent(ui),
                        13.0,
                    );
                    x += width;
                }
                ui.spacing_mut().item_spacing.y = 0.0;
                for (index, field) in file.fields.iter().enumerate() {
                    let (rect, response) =
                        ui.allocate_exact_size(egui::vec2(total, 24.0), Sense::hover());
                    let end = field.offset.saturating_add(field.size);
                    let selected = response.hovered()
                        || self
                            .hex_hover_byte
                            .is_some_and(|byte| (field.offset..end).contains(&byte))
                        || self
                            .hex_selection
                            .is_some_and(|(a, b)| a < end && b > field.offset);
                    let fill = if selected {
                        selection_fill(ui)
                    } else if index % 2 == 0 {
                        stripe(ui)
                    } else {
                        Color32::TRANSPARENT
                    };
                    ui.painter().rect_filled(rect, 0.0, fill);
                    let mut x = rect.left();
                    for (value, width) in [
                        field.name.clone(),
                        format!("0x{:08X}", field.offset),
                        field.size.to_string(),
                        field.value.clone(),
                    ]
                    .iter()
                    .zip(widths)
                    {
                        clipped_text(
                            ui,
                            Rect::from_min_size(Pos2::new(x, rect.top()), egui::vec2(width, 24.0)),
                            value,
                            selected_text(selected, ui),
                            12.0,
                        );
                        x += width;
                    }
                    if response.hovered() && field.size > 0 && field.offset < file.bytes.len() {
                        hovered = Some((field.offset, end.min(file.bytes.len())));
                    }
                    response.on_hover_text(&field.value);
                }
            });
        if hovered != self.hex_hover_field {
            if let Some((start, _)) = hovered {
                self.hex_scroll_to = Some(start);
            }
            self.hex_hover_field = hovered;
            ui.ctx().request_repaint();
        }
    }

    fn hex_bytes(&mut self, ui: &mut egui::Ui, bytes: &[u8]) {
        let mut hovered = None;
        let row_height = 17.0;
        let cell_width = 22.0;
        let ascii_width = 7.8;
        let byte_start = 76.0;
        let ascii_start = byte_start + 16.0 * cell_width + 10.0;
        let total = ascii_start + 16.0 * ascii_width + 8.0;
        let font = FontId::monospace(13.0);
        egui::ScrollArea::horizontal()
            .id_salt("hex-horizontal")
            .auto_shrink([false, false])
            .show_themed(ui, |ui| {
                let (header, _) = ui.allocate_exact_size(egui::vec2(total, 23.0), Sense::hover());
                ui.painter().text(
                    header.left_center(),
                    Align2::LEFT_CENTER,
                    "ADDRESS",
                    font.clone(),
                    accent(ui),
                );
                for column in 0..16 {
                    ui.painter().text(
                        Pos2::new(
                            header.left() + byte_start + column as f32 * cell_width,
                            header.center().y,
                        ),
                        Align2::LEFT_CENTER,
                        format!("{column:02X}"),
                        font.clone(),
                        accent(ui),
                    );
                    ui.painter().text(
                        Pos2::new(
                            header.left() + ascii_start + column as f32 * ascii_width,
                            header.center().y,
                        ),
                        Align2::LEFT_CENTER,
                        format!("{column:X}"),
                        font.clone(),
                        accent(ui),
                    );
                }
                ui.painter().hline(
                    header.x_range(),
                    header.bottom(),
                    Stroke::new(1.0_f32, rule_color(ui)),
                );
                ui.spacing_mut().item_spacing.y = 0.0;
                let mut scroll = egui::ScrollArea::vertical()
                    .id_salt("hex-rows")
                    .auto_shrink([false, false]);
                if let Some(offset) = self.hex_scroll_to.take() {
                    scroll = scroll.vertical_scroll_offset((offset / 16) as f32 * row_height);
                }
                scroll.show_rows_themed(ui, row_height, bytes.len().div_ceil(16), |ui, rows| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for row in rows {
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(total, row_height), Sense::hover());
                        ui.painter().text(
                            rect.left_center(),
                            Align2::LEFT_CENTER,
                            format!("{:08X}", row * 16),
                            font.clone(),
                            muted(ui),
                        );
                        for column in 0..16 {
                            let index = row * 16 + column;
                            let Some(byte) = bytes.get(index) else { break };
                            let hex_rect = Rect::from_min_size(
                                Pos2::new(
                                    rect.left() + byte_start + column as f32 * cell_width,
                                    rect.top(),
                                ),
                                egui::vec2(cell_width, row_height),
                            );
                            let ascii_rect = Rect::from_min_size(
                                Pos2::new(
                                    rect.left() + ascii_start + column as f32 * ascii_width,
                                    rect.top(),
                                ),
                                egui::vec2(ascii_width, row_height),
                            );
                            for (part, target) in [("hex", hex_rect), ("ascii", ascii_rect)] {
                                let response = ui.interact(
                                    target,
                                    ui.id().with((part, index)),
                                    Sense::click_and_drag(),
                                );
                                if response.hovered() {
                                    hovered = Some(index);
                                }
                                if response.clicked() || response.drag_started() {
                                    let anchor = if ui.input(|i| i.modifiers.shift) {
                                        self.hex_anchor.unwrap_or(index)
                                    } else {
                                        index
                                    };
                                    self.hex_anchor = Some(anchor);
                                    self.hex_selection =
                                        Some((anchor.min(index), anchor.max(index) + 1));
                                }
                                if response.contains_pointer()
                                    && ui.input(|i| i.pointer.primary_down())
                                {
                                    if let Some(anchor) = self.hex_anchor {
                                        self.hex_selection =
                                            Some((anchor.min(index), anchor.max(index) + 1));
                                    }
                                }
                                response.on_hover_text(format!(
                                    "Offset 0x{index:08X}\n0x{byte:02X} · {byte}"
                                ));
                            }
                            let selected = hovered == Some(index)
                                || self
                                    .hex_hover_field
                                    .is_some_and(|(a, b)| (a..b).contains(&index))
                                || self
                                    .hex_selection
                                    .is_some_and(|(a, b)| (a..b).contains(&index));
                            if selected {
                                ui.painter().rect_filled(hex_rect, 0.0, selection_fill(ui));
                                ui.painter()
                                    .rect_filled(ascii_rect, 0.0, selection_fill(ui));
                            }
                            let color = selected_text(selected, ui);
                            ui.painter().text(
                                hex_rect.left_center(),
                                Align2::LEFT_CENTER,
                                format!("{byte:02X}"),
                                font.clone(),
                                color,
                            );
                            let ascii = if (0x20..=0x7e).contains(byte) {
                                *byte as char
                            } else {
                                '.'
                            };
                            ui.painter().text(
                                ascii_rect.left_center(),
                                Align2::LEFT_CENTER,
                                ascii,
                                font.clone(),
                                color,
                            );
                        }
                    }
                });
            });
        if self.hex_hover_byte != hovered {
            self.hex_hover_byte = hovered;
            ui.ctx().request_repaint();
        }
    }
}
