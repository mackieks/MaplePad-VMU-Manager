//! Transparent client-side caption with native window movement and resizing.
use eframe::egui::{
    self, Align2, Color32, FontId, Pos2, Rect, Sense, TextureHandle, Vec2, ViewportCommand,
};

pub const HEIGHT: f32 = 30.0;
const BUTTON_WIDTH: f32 = 46.0;

fn caption_icon_pixels(ppp: f32) -> u32 {
    (16.0 * ppp).round().clamp(1.0, 256.0) as u32
}
fn caption_raster(master: &image::RgbaImage, pixels: u32) -> egui::ColorImage {
    // Same Lanczos filter and source as build.rs uses for the Windows ICO.
    // GPU bilinear sampling alone skips most texels when reducing 256px to 16px.
    let rgba = image::imageops::resize(
        master,
        pixels,
        pixels,
        image::imageops::FilterType::Lanczos3,
    );
    egui::ColorImage::from_rgba_unmultiplied([pixels as usize, pixels as usize], rgba.as_raw())
}

fn control_pixels(ppp: f32) -> usize {
    (10.0 * ppp).round().clamp(8.0, 80.0) as usize
}

fn control_raster(kind: usize, size: usize) -> egui::ColorImage {
    let mut image = egui::ColorImage::new([size, size], Color32::TRANSPARENT);
    for y in 0..size {
        for x in 0..size {
            let alpha = match kind {
                0 => {
                    if y == size / 2 + 2 {
                        255
                    } else {
                        0
                    }
                }
                // Exact one-physical-pixel border, including at fractional DPI.
                1 => {
                    if x == 0 || y == 0 || x == size - 1 || y == size - 1 {
                        255
                    } else {
                        0
                    }
                }
                2 => {
                    let inset = (size / 5).max(2);
                    if (y == 0 && x >= inset)
                        || (x == size - 1 && y < size - inset)
                        || (y == inset && x < size - inset)
                        || (x == 0 && y >= inset)
                        || (y == size - 1 && x < size - inset)
                        || (x == size - inset - 1 && y >= inset)
                    {
                        255
                    } else {
                        0
                    }
                }
                _ => {
                    // Sample both diagonals together. Mirror symmetry is exact;
                    // the crossing never gets a second layer of alpha blending.
                    let mut covered = 0;
                    for sy in 0..8 {
                        for sx in 0..8 {
                            let px = x as f32 + (sx as f32 + 0.5) / 8.0;
                            let py = y as f32 + (sy as f32 + 0.5) / 8.0;
                            if (px - py).abs().min((px + py - size as f32).abs())
                                <= std::f32::consts::FRAC_1_SQRT_2
                            {
                                covered += 1;
                            }
                        }
                    }
                    (covered * 255 / 64) as u8
                }
            };
            image[(x, y)] = Color32::from_white_alpha(alpha);
        }
    }
    image
}

pub struct TitleBar {
    icon: TextureHandle,
    icon_master: image::RgbaImage,
    icon_pixels: u32,
    controls: [TextureHandle; 4],
    control_pixels: usize,
}
impl TitleBar {
    pub fn new(ctx: &egui::Context) -> Self {
        let icon_master =
            image::load_from_memory(include_bytes!("../assets/maple-icon-master.png"))
                .expect("caption icon master")
                .to_rgba8();
        let icon_pixels = caption_icon_pixels(ctx.pixels_per_point());
        let icon = caption_raster(&icon_master, icon_pixels);
        let control_pixels = control_pixels(ctx.pixels_per_point());
        let controls = std::array::from_fn(|kind| {
            ctx.load_texture(
                format!("caption-control-{kind}"),
                control_raster(kind, control_pixels),
                egui::TextureOptions::NEAREST,
            )
        });
        Self {
            controls,
            control_pixels,
            icon: ctx.load_texture("caption-maple", icon, egui::TextureOptions::NEAREST),
            icon_master,
            icon_pixels,
        }
    }
    pub fn show(&mut self, ctx: &egui::Context) {
        if self.show_named(ctx, "MaplePad VMU Manager", true) {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
    }
    /// Returns Close to the owner so child windows cancel their own editor only.
    pub fn show_named(&mut self, ctx: &egui::Context, title: &str, show_icon: bool) -> bool {
        let mut close = false;
        let pixels = caption_icon_pixels(ctx.pixels_per_point());
        if pixels != self.icon_pixels {
            self.icon.set(
                caption_raster(&self.icon_master, pixels),
                egui::TextureOptions::NEAREST,
            );
            self.icon_pixels = pixels;
        }
        let control_pixels = control_pixels(ctx.pixels_per_point());
        if control_pixels != self.control_pixels {
            for (kind, texture) in self.controls.iter_mut().enumerate() {
                texture.set(
                    control_raster(kind, control_pixels),
                    egui::TextureOptions::NEAREST,
                );
            }
            self.control_pixels = control_pixels;
        }
        let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        egui::TopBottomPanel::top("caption")
            .show_separator_line(false)
            .exact_height(HEIGHT)
            .frame(egui::Frame::none())
            .show(ctx, |ui| {
                let rect = ui.max_rect();
                let foreground = ui.visuals().text_color();
                let controls_left = rect.right() - 3.0 * BUTTON_WIDTH;
                let drag_rect = Rect::from_min_max(
                    rect.min + egui::vec2(5.0, 5.0),
                    Pos2::new(controls_left, rect.bottom()),
                );
                let drag = ui.interact(drag_rect, ui.id().with("drag"), Sense::click_and_drag());
                if drag.double_clicked() {
                    ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized));
                } else if drag.drag_started() {
                    ctx.send_viewport_cmd(ViewportCommand::StartDrag);
                }
                // Already antialiased at the final physical size: draw 1:1 on
                // pixel boundaries, avoiding both minification and half-pixel blur.
                let icon_size = Vec2::splat(self.icon_pixels as f32 / ctx.pixels_per_point());
                let icon_min = ui.painter().round_pos_to_pixels(
                    Pos2::new(rect.left() + 17.0, rect.center().y) - icon_size / 2.0,
                );
                let icon_rect = Rect::from_min_size(icon_min, icon_size);
                if show_icon {
                    ui.painter().image(
                        self.icon.id(),
                        icon_rect,
                        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                        Color32::WHITE,
                    );
                }
                ui.painter().text(
                    Pos2::new(
                        rect.left() + if show_icon { 31.0 } else { 9.0 },
                        rect.center().y,
                    ),
                    Align2::LEFT_CENTER,
                    title,
                    FontId::proportional(12.0),
                    foreground,
                );
                for (n, label) in [
                    "Minimize",
                    if maximized { "Restore" } else { "Maximize" },
                    "Close",
                ]
                .iter()
                .enumerate()
                {
                    let button = Rect::from_min_size(
                        Pos2::new(controls_left + n as f32 * BUTTON_WIDTH, rect.top()),
                        Vec2::new(BUTTON_WIDTH, HEIGHT),
                    );
                    let response = ui
                        .interact(button, ui.id().with(n), Sense::click())
                        .on_hover_text(*label);
                    let mut color = if ui.visuals().dark_mode {
                        Color32::WHITE
                    } else {
                        Color32::BLACK
                    };
                    if response.hovered() || response.is_pointer_button_down_on() {
                        let fill = if n == 2 {
                            color = Color32::WHITE;
                            Color32::from_rgb(179, 32, 40)
                        } else {
                            ui.visuals().selection.bg_fill.gamma_multiply(0.65)
                        };
                        ui.painter().rect_filled(button.shrink(2.0), 2.0, fill);
                    }
                    let ppp = ctx.pixels_per_point();
                    let glyph = match n {
                        0 => 0,
                        1 if maximized => 2,
                        1 => 1,
                        _ => 3,
                    };
                    let size = Vec2::splat(self.control_pixels as f32 / ppp);
                    let min = ui
                        .painter()
                        .round_pos_to_pixels(button.center() - size / 2.0);
                    ui.painter().image(
                        self.controls[glyph].id(),
                        Rect::from_min_size(min, size),
                        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                        color,
                    );
                    if response.clicked() {
                        match n {
                            0 => {
                                if show_icon || !crate::native_animation::minimize_editor_group() {
                                    ctx.send_viewport_cmd(ViewportCommand::Minimized(true));
                                }
                            }
                            1 => ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized)),
                            _ => close = true,
                        }
                    }
                }
            });
        close
    }
}

/// Borderless windows still use Windows' resize loop. Reserve the same five
/// pixels for hit testing and cursor feedback, including larger corner targets.
pub fn resize_edges(ctx: &egui::Context) {
    if ctx.input(|i| i.viewport().maximized.unwrap_or(false)) {
        return;
    }
    let screen = ctx.screen_rect();
    let Some(pos) = ctx.pointer_hover_pos() else {
        return;
    };
    let Some(direction) = resize_direction(screen, pos) else {
        return;
    };
    use egui::ResizeDirection::*;
    ctx.set_cursor_icon(match direction {
        North | South => egui::CursorIcon::ResizeVertical,
        East | West => egui::CursorIcon::ResizeHorizontal,
        NorthEast | SouthWest => egui::CursorIcon::ResizeNeSw,
        _ => egui::CursorIcon::ResizeNwSe,
    });
    if ctx.input(|i| i.pointer.button_pressed(egui::PointerButton::Primary)) {
        ctx.send_viewport_cmd(ViewportCommand::BeginResize(direction));
    }
}
fn resize_direction(rect: Rect, pos: Pos2) -> Option<egui::ResizeDirection> {
    use egui::ResizeDirection::*;
    if !rect.contains(pos) {
        return None;
    }
    let left = pos.x - rect.left();
    let right = rect.right() - pos.x;
    let top = pos.y - rect.top();
    let bottom = rect.bottom() - pos.y;
    if left.min(right).min(top).min(bottom) > 5.0 {
        return None;
    }
    Some(if top <= 12.0 && left <= 12.0 {
        NorthWest
    } else if top <= 12.0 && right <= 12.0 {
        NorthEast
    } else if bottom <= 12.0 && left <= 12.0 {
        SouthWest
    } else if bottom <= 12.0 && right <= 12.0 {
        SouthEast
    } else if top <= 5.0 {
        North
    } else if bottom <= 5.0 {
        South
    } else if left <= 5.0 {
        West
    } else {
        East
    })
}

pub(crate) fn native_hit(size: Vec2, pos: Pos2, maximized: bool) -> Option<isize> {
    use egui::ResizeDirection::*;
    let rect = Rect::from_min_size(Pos2::ZERO, size);
    if !maximized {
        if let Some(direction) = resize_direction(rect, pos) {
            return Some(match direction {
                West => 10,
                East => 11,
                North => 12,
                NorthWest => 13,
                NorthEast => 14,
                South => 15,
                SouthWest => 16,
                SouthEast => 17,
            });
        }
    }
    // HTCAPTION gives Windows control of dragging, double-click and snap;
    // custom caption buttons remain regular client widgets.
    if rect.contains(pos) && pos.y < HEIGHT && pos.x < size.x - 3.0 * BUTTON_WIDTH {
        Some(2)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn caption_masks_are_symmetric_and_maximize_is_one_opaque_pixel_at_every_scale() {
        for size in [8, 10, 12, 13, 15, 20, 30] {
            let close = control_raster(3, size);
            let maximize = control_raster(1, size);
            for y in 0..size {
                for x in 0..size {
                    assert_eq!(close[(x, y)], close[(size - 1 - x, y)]);
                    assert_eq!(close[(x, y)], close[(x, size - 1 - y)]);
                    assert_eq!(
                        maximize[(x, y)],
                        if x == 0 || y == 0 || x == size - 1 || y == size - 1 {
                            Color32::WHITE
                        } else {
                            Color32::TRANSPARENT
                        }
                    );
                }
            }
        }
    }
    #[test]
    fn border_hit_targets_leave_caption_controls_and_workspace_available() {
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(1200.0, 860.0));
        assert_eq!(
            resize_direction(rect, Pos2::new(2.0, 10.0)),
            Some(egui::ResizeDirection::NorthWest)
        );
        assert_eq!(
            resize_direction(rect, Pos2::new(1198.0, 858.0)),
            Some(egui::ResizeDirection::SouthEast)
        );
        assert_eq!(
            resize_direction(rect, Pos2::new(600.0, 2.0)),
            Some(egui::ResizeDirection::North)
        );
        assert_eq!(resize_direction(rect, Pos2::new(1190.0, 15.0)), None);
        assert_eq!(resize_direction(rect, Pos2::new(600.0, 300.0)), None);
        assert_eq!(
            native_hit(rect.size(), Pos2::new(600.0, 15.0), false),
            Some(2)
        );
        assert_eq!(
            native_hit(rect.size(), Pos2::new(1130.0, 15.0), false),
            None
        );
        assert_eq!(
            native_hit(rect.size(), Pos2::new(1198.0, 600.0), false),
            Some(11)
        );
        assert_eq!(
            native_hit(rect.size(), Pos2::new(1198.0, 600.0), true),
            None
        );
    }
}
