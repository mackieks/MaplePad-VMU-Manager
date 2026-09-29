use eframe::egui::{self, Color32, Stroke};
use std::{fs, path::PathBuf};

pub fn card_fill(root: [u8; 4], background: Color32, light: bool) -> Color32 {
    let alpha = root[3] as f32 / 255.0 * if light { 0.40 } else { 0.24 };
    // Composite once in display RGB. This preserves the root hue on cream and
    // avoids an additive-looking, nearly white low-alpha fill in the renderer.
    let channel =
        |value: u8, base: u8| (base as f32 * (1.0 - alpha) + value as f32 * alpha).round() as u8;
    Color32::from_rgb(
        channel(root[0], background.r()),
        channel(root[1], background.g()),
        channel(root[2], background.b()),
    )
}

// ScrollArea takes its thumb color from general widget visuals. Apply the
// crimson palette only to the scroll frame, restoring content styling inside.
fn scroll_style(ui: &mut egui::Ui) {
    let light = !ui.visuals().dark_mode;
    ui.spacing_mut().scroll.foreground_color = false;
    let v = ui.visuals_mut();
    v.widgets.inactive.bg_fill = if light {
        Color32::from_rgb(173, 83, 77)
    } else {
        Color32::from_rgb(111, 35, 43)
    };
    v.widgets.hovered.bg_fill = if light {
        Color32::from_rgb(150, 44, 43)
    } else {
        Color32::from_rgb(175, 34, 50)
    };
    v.widgets.active.bg_fill = if light {
        Color32::from_rgb(128, 30, 32)
    } else {
        Color32::from_rgb(205, 24, 43)
    };
}
pub trait ThemedScrollArea {
    fn show_themed<R>(
        self,
        ui: &mut egui::Ui,
        add: impl FnOnce(&mut egui::Ui) -> R,
    ) -> egui::scroll_area::ScrollAreaOutput<R>;
    fn show_rows_themed<R>(
        self,
        ui: &mut egui::Ui,
        height: f32,
        count: usize,
        add: impl FnOnce(&mut egui::Ui, std::ops::Range<usize>) -> R,
    ) -> egui::scroll_area::ScrollAreaOutput<R>;
}
impl ThemedScrollArea for egui::ScrollArea {
    fn show_themed<R>(
        self,
        ui: &mut egui::Ui,
        add: impl FnOnce(&mut egui::Ui) -> R,
    ) -> egui::scroll_area::ScrollAreaOutput<R> {
        let style = ui.style().clone();
        scroll_style(ui);
        let result = self.show(ui, |child| {
            child.set_style(style.clone());
            add(child)
        });
        ui.set_style(style);
        result
    }
    fn show_rows_themed<R>(
        self,
        ui: &mut egui::Ui,
        height: f32,
        count: usize,
        add: impl FnOnce(&mut egui::Ui, std::ops::Range<usize>) -> R,
    ) -> egui::scroll_area::ScrollAreaOutput<R> {
        let style = ui.style().clone();
        scroll_style(ui);
        let result = self.show_rows(ui, height, count, |child, rows| {
            child.set_style(style.clone());
            add(child, rows)
        });
        ui.set_style(style);
        result
    }
}

fn path() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|p| {
        PathBuf::from(p)
            .join("MaplePad VMU Manager")
            .join("theme.txt")
    })
}
pub fn load_light() -> bool {
    path()
        .and_then(|p| fs::read_to_string(p).ok())
        .is_some_and(|s| s.trim() == "light")
}
pub fn save(light: bool) -> Result<(), String> {
    if let Some(path) = path() {
        fs::create_dir_all(path.parent().unwrap())
            .and_then(|_| fs::write(path, if light { "light" } else { "dark" }))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
pub fn apply(ctx: &egui::Context, light: bool) {
    let mut v = if light {
        egui::Visuals::light()
    } else {
        egui::Visuals::dark()
    };
    let red = Color32::from_rgb(205, 24, 43);
    let selected = if light {
        Color32::from_rgb(220, 164, 158)
    } else {
        Color32::from_rgb(100, 20, 32)
    };
    let accent = if light {
        Color32::from_rgb(150, 44, 43)
    } else {
        Color32::from_rgb(242, 70, 88)
    };
    let selected_text = if light {
        Color32::from_rgb(77, 25, 24)
    } else {
        Color32::WHITE
    };
    let text = if light {
        Color32::from_rgb(48, 37, 32)
    } else {
        Color32::from_rgb(215, 215, 220)
    };
    let line = if light {
        Color32::from_rgb(204, 189, 168)
    } else {
        Color32::from_rgb(52, 52, 56)
    };
    v.panel_fill = if light {
        Color32::from_rgb(246, 238, 220)
    } else {
        Color32::from_rgb(20, 20, 23)
    };
    v.window_fill = if light {
        Color32::from_rgb(251, 244, 230)
    } else {
        Color32::from_rgb(26, 26, 29)
    };
    v.extreme_bg_color = if light {
        Color32::from_rgb(235, 223, 201)
    } else {
        Color32::from_rgb(12, 12, 15)
    };
    // Let active/open widgets use their own contrasting foreground colors.
    v.override_text_color = None;
    v.selection.bg_fill = selected;
    v.selection.stroke = Stroke::new(1.0_f32, if light { selected_text } else { accent });
    v.widgets.noninteractive.fg_stroke.color = text;
    v.widgets.noninteractive.bg_stroke.color = line;
    v.widgets.inactive.fg_stroke.color = text;
    v.widgets.inactive.bg_fill = if light {
        Color32::from_rgb(231, 217, 195)
    } else {
        Color32::from_rgb(43, 43, 48)
    };
    v.widgets.inactive.weak_bg_fill = v.widgets.inactive.bg_fill;
    // A zero-width stroke with an opaque color still enters epaint's stroke
    // tessellation path. Use NONE, not just a changed color, for flat buttons.
    v.widgets.inactive.bg_stroke = Stroke::NONE;
    v.widgets.hovered.bg_fill = if light {
        Color32::from_rgb(239, 207, 199)
    } else {
        Color32::from_rgb(66, 26, 35)
    };
    v.widgets.hovered.weak_bg_fill = v.widgets.hovered.bg_fill;
    v.widgets.hovered.fg_stroke.color = text;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, accent);
    v.widgets.active.bg_fill = if light {
        Color32::from_rgb(208, 141, 133)
    } else {
        red
    };
    v.widgets.active.weak_bg_fill = v.widgets.active.bg_fill;
    v.widgets.active.fg_stroke.color = selected_text;
    v.widgets.active.bg_stroke = Stroke::new(1.0_f32, accent);
    v.widgets.open.bg_fill = selected;
    v.widgets.open.weak_bg_fill = selected;
    v.widgets.open.fg_stroke.color = selected_text;
    v.widgets.open.bg_stroke = Stroke::new(1.0_f32, accent);
    let mut style = ctx.style().as_ref().clone();
    style.visuals = v;
    ctx.set_theme(if light {
        egui::Theme::Light
    } else {
        egui::Theme::Dark
    });
    ctx.set_style(style);
    crate::ui::apply_rounding(ctx);
}
