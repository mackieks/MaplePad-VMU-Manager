//! Windows runs its own message loop during a border/title-bar grab. Wake WM_PAINT
//! from that loop, rather than relying on winit's outer-loop repaint deadline.
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::cell::Cell;
use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering};

static UI_SCALE: AtomicU32 = AtomicU32::new(0);
pub fn set_scale(pixels_per_point: f32) {
    UI_SCALE.store(pixels_per_point.to_bits(), Ordering::Relaxed);
}
type Hwnd = *mut c_void;
thread_local! {
    static MAIN_WINDOW: Cell<isize> = const { Cell::new(0) };
    static EDITOR_WINDOW: Cell<isize> = const { Cell::new(0) };
    // Cache before SetWindowRgn, which can synchronously send position messages.
    static CORNER_REGION: Cell<Option<(usize, i32, i32, i32, bool)>> = const { Cell::new(None) };
}
#[repr(C)]
struct Point {
    x: i32,
    y: i32,
}
#[repr(C)]
struct WindowRect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}
type Callback = unsafe extern "system" fn(Hwnd, u32, usize, isize, usize, usize) -> isize;
const ID: usize = 0x4d41504c;
const FOCUS_EDITOR: u32 = 0x8000 + 0x4d;
#[link(name = "comctl32")]
extern "system" {
    fn SetWindowSubclass(hwnd: Hwnd, callback: Callback, id: usize, data: usize) -> i32;
    fn RemoveWindowSubclass(hwnd: Hwnd, callback: Callback, id: usize) -> i32;
    fn DefSubclassProc(hwnd: Hwnd, msg: u32, w: usize, l: isize) -> isize;
}
#[link(name = "user32")]
extern "system" {
    fn GetActiveWindow() -> Hwnd;
    fn GetWindowTextW(hwnd: Hwnd, text: *mut u16, capacity: i32) -> i32;
    fn SetWindowLongPtrW(hwnd: Hwnd, index: i32, value: isize) -> isize;
    fn GetWindowLongPtrW(hwnd: Hwnd, index: i32) -> isize;
    fn EnableWindow(hwnd: Hwnd, enabled: i32) -> i32;
    fn SetActiveWindow(hwnd: Hwnd) -> Hwnd;
    fn SetFocus(hwnd: Hwnd) -> Hwnd;
    fn PostMessageW(hwnd: Hwnd, message: u32, w: usize, l: isize) -> i32;
    fn GetLastActivePopup(hwnd: Hwnd) -> Hwnd;
    fn IsWindow(hwnd: Hwnd) -> i32;
    fn ShowWindow(hwnd: Hwnd, command: i32) -> i32;
    fn SetWindowPos(
        hwnd: Hwnd,
        after: Hwnd,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        flags: u32,
    ) -> i32;
    fn SetTimer(hwnd: Hwnd, id: usize, ms: u32, callback: usize) -> usize;
    fn KillTimer(hwnd: Hwnd, id: usize) -> i32;
    fn RedrawWindow(hwnd: Hwnd, rect: *const c_void, region: Hwnd, flags: u32) -> i32;
    fn GetClientRect(hwnd: Hwnd, rect: *mut WindowRect) -> i32;
    fn ScreenToClient(hwnd: Hwnd, point: *mut Point) -> i32;
    fn GetDpiForWindow(hwnd: Hwnd) -> u32;
    fn IsZoomed(hwnd: Hwnd) -> i32;
    fn IsIconic(hwnd: Hwnd) -> i32;
    fn GetWindowRect(hwnd: Hwnd, rect: *mut WindowRect) -> i32;
    fn SetWindowRgn(hwnd: Hwnd, region: Hwnd, redraw: i32) -> i32;
}
#[link(name = "gdi32")]
extern "system" {
    fn CreateRoundRectRgn(
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
        ellipse_width: i32,
        ellipse_height: i32,
    ) -> Hwnd;
    fn DeleteObject(object: Hwnd) -> i32;
}

unsafe fn round_window(hwnd: Hwnd) {
    if IsIconic(hwnd) != 0 {
        return;
    }
    let mut rect = WindowRect {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    if GetWindowRect(hwnd, &mut rect) == 0 {
        return;
    }
    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;
    if width <= 0 || height <= 0 {
        return;
    }
    let diameter = (4.0 * GetDpiForWindow(hwnd).max(96) as f32 / 96.0).round() as i32;
    let maximized = IsZoomed(hwnd) != 0;
    let key = (hwnd as usize, width, height, diameter, maximized);
    if CORNER_REGION.with(|cache| {
        if cache.get() == Some(key) {
            true
        } else {
            cache.set(Some(key));
            false
        }
    }) {
        return;
    }
    // Full screen edges stay square when maximized, as on standard Windows apps.
    let region = if maximized {
        std::ptr::null_mut()
    } else {
        CreateRoundRectRgn(0, 0, width + 1, height + 1, diameter, diameter)
    };
    if !maximized && region.is_null() {
        CORNER_REGION.with(|c| c.set(None));
        return;
    }
    if SetWindowRgn(hwnd, region, 1) == 0 {
        if !region.is_null() {
            DeleteObject(region);
        }
        CORNER_REGION.with(|c| c.set(None));
    }
    // On success Windows owns and releases the region.
}

unsafe extern "system" fn callback(
    hwnd: Hwnd,
    msg: u32,
    w: usize,
    l: isize,
    _: usize,
    _: usize,
) -> isize {
    match msg {
        0x0006 if w & 0xffff != 0 && MAIN_WINDOW.with(|m| m.get()) == hwnd as isize => {
            // Let the shell finish activating/restoring the enabled taskbar
            // representative before moving focus to its editor. Doing this
            // synchronously lets DefWindowProc steal keyboard focus back.
            if EDITOR_WINDOW.with(|e| e.get()) != 0 {
                PostMessageW(hwnd, FOCUS_EDITOR, 0, 0);
            }
        }
        FOCUS_EDITOR => {
            focus_editor(hwnd);
            return 0;
        }
        0x0021 | 0x00a1
            if MAIN_WINDOW.with(|m| m.get()) == hwnd as isize
                && EDITOR_WINDOW.with(|e| e.get()) != 0 =>
        {
            // WM_MOUSEACTIVATE / WM_NCLBUTTONDOWN: keep the owner available to
            // Alt+Tab, but consume clicks on its dimmed client and caption.
            focus_editor(hwnd);
            return if msg == 0x0021 { 4 } else { 0 }; // MA_NOACTIVATEANDEAT
        }
        0x0005 | 0x02e0 => {
            // WM_SIZE / WM_DPICHANGED
            let result = DefSubclassProc(hwnd, msg, w, l);
            round_window(hwnd);
            return result;
        }
        0x0084 => {
            // WM_NCHITTEST: start move/resize immediately on mouse-down,
            // without waiting for egui's next frame to issue a command.
            let mut point = Point {
                x: (l as u16 as i16) as i32,
                y: ((l >> 16) as u16 as i16) as i32,
            };
            let mut rect = WindowRect {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            };
            if ScreenToClient(hwnd, &mut point) != 0 && GetClientRect(hwnd, &mut rect) != 0 {
                let ui_scale = f32::from_bits(UI_SCALE.load(Ordering::Relaxed));
                let scale = if ui_scale > 0.0 {
                    ui_scale
                } else {
                    GetDpiForWindow(hwnd).max(96) as f32 / 96.0
                };
                let size = eframe::egui::vec2(
                    (rect.right - rect.left) as f32 / scale,
                    (rect.bottom - rect.top) as f32 / scale,
                );
                let pos = eframe::egui::pos2(point.x as f32 / scale, point.y as f32 / scale);
                if let Some(hit) = crate::titlebar::native_hit(size, pos, IsZoomed(hwnd) != 0) {
                    return hit;
                }
            }
        }
        0x0231 => {
            SetTimer(hwnd, ID, 16, 0);
        } // WM_ENTERSIZEMOVE
        0x0113 if w == ID => {
            // RDW_INTERNALPAINT: asynchronous paint, avoiding nested egui rendering.
            RedrawWindow(hwnd, std::ptr::null(), std::ptr::null_mut(), 2);
            return 0;
        }
        0x0232 => {
            KillTimer(hwnd, ID);
        } // WM_EXITSIZEMOVE
        0x0082 => {
            // WM_NCDESTROY
            KillTimer(hwnd, ID);
            RemoveWindowSubclass(hwnd, callback, ID);
            CORNER_REGION.with(|c| c.set(None));
        }
        _ => {}
    }
    DefSubclassProc(hwnd, msg, w, l)
}

unsafe fn focus_editor(owner: Hwnd) {
    let editor = EDITOR_WINDOW.with(|e| e.get()) as Hwnd;
    if editor.is_null() || IsWindow(editor) == 0 {
        return;
    }
    if IsIconic(owner) != 0 {
        ShowWindow(owner, 9);
    }
    if IsIconic(editor) != 0 {
        ShowWindow(editor, 9);
    }
    let popup = GetLastActivePopup(editor);
    SetActiveWindow(popup);
    SetFocus(popup);
}

pub fn install(cc: &eframe::CreationContext<'_>) {
    if let Ok(handle) = cc.window_handle() {
        if let RawWindowHandle::Win32(handle) = handle.as_raw() {
            // Installed on the owning UI thread; removed when the HWND is destroyed.
            unsafe {
                MAIN_WINDOW.with(|m| m.set(handle.hwnd.get()));
                SetWindowSubclass(handle.hwnd.get() as Hwnd, callback, ID, 0);
                round_window(handle.hwnd.get() as Hwnd);
            }
        }
    }
}

#[derive(Clone, Copy)]
pub struct EditorWindow(isize);
impl HasWindowHandle for EditorWindow {
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        let hwnd = std::num::NonZeroIsize::new(self.0)
            .ok_or(raw_window_handle::HandleError::Unavailable)?;
        let raw = RawWindowHandle::Win32(raw_window_handle::Win32WindowHandle::new(hwnd));
        Ok(unsafe { raw_window_handle::WindowHandle::borrow_raw(raw) })
    }
}
impl raw_window_handle::HasDisplayHandle for EditorWindow {
    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        Ok(raw_window_handle::DisplayHandle::windows())
    }
}
/// eframe 0.29 exposes a handle for the main viewport only. GetActiveWindow
/// returns this UI thread's active HWND; check its title before subclassing.
pub fn install_editor(ctx: &eframe::egui::Context) -> Option<EditorWindow> {
    unsafe {
        let hwnd = GetActiveWindow();
        let mut title = [0u16; 64];
        let len = GetWindowTextW(hwnd, title.as_mut_ptr(), title.len() as i32);
        if len <= 0 || String::from_utf16_lossy(&title[..len as usize]) != "Edit Root Block" {
            return None;
        }
        if SetWindowSubclass(hwnd, callback, ID, 0) == 0 {
            return None;
        }
        let owner = MAIN_WINDOW.with(|m| m.get()) as Hwnd;
        if !owner.is_null() {
            SetWindowLongPtrW(hwnd, -8, owner as isize); // GWLP_HWNDPARENT: owner, not child clipping
            let style = GetWindowLongPtrW(hwnd, -20);
            SetWindowLongPtrW(hwnd, -20, (style & !0x00040000) | 0x00000080);
            SetWindowPos(hwnd, std::ptr::null_mut(), 0, 0, 0, 0, 0x0037);
            EDITOR_WINDOW.with(|e| e.set(hwnd as isize));
            // A disabled owner cannot be activated from Alt+Tab once its owned
            // tool window is hidden by minimization. Keep the native owner
            // enabled; egui and WM_MOUSEACTIVATE enforce editor modality.
            EnableWindow(owner, 1);
        }
        round_window(hwnd);
        let window = EditorWindow(hwnd as isize);
        title_colors(
            &window,
            ctx.style().visuals.panel_fill,
            ctx.style().visuals.text_color(),
            ctx.style().visuals.dark_mode,
        );
        Some(window)
    }
}

pub fn release_editor(window: EditorWindow) {
    if EDITOR_WINDOW.with(|e| {
        if e.get() == window.0 {
            e.set(0);
            true
        } else {
            false
        }
    }) {
        let owner = MAIN_WINDOW.with(|m| m.get()) as Hwnd;
        unsafe {
            if !owner.is_null() && IsWindow(owner) != 0 {
                EnableWindow(owner, 1);
                SetActiveWindow(owner);
            }
        }
    }
}

pub fn minimize_editor_group() -> bool {
    let owner = MAIN_WINDOW.with(|m| m.get()) as Hwnd;
    if EDITOR_WINDOW.with(|e| e.get()) != 0 && !owner.is_null() {
        unsafe {
            ShowWindow(owner, 6);
        }
        true
    } else {
        false
    }
}

pub fn title_colors(
    window: &impl HasWindowHandle,
    background: eframe::egui::Color32,
    text: eframe::egui::Color32,
    dark: bool,
) {
    #[link(name = "dwmapi")]
    extern "system" {
        fn DwmSetWindowAttribute(
            hwnd: Hwnd,
            attribute: u32,
            value: *const c_void,
            size: u32,
        ) -> i32;
    }
    if let Ok(handle) = window.window_handle() {
        if let RawWindowHandle::Win32(handle) = handle.as_raw() {
            let colorref = |c: eframe::egui::Color32| {
                u32::from(c.r()) | (u32::from(c.g()) << 8) | (u32::from(c.b()) << 16)
            };
            // Exact caption/border colors on Windows 11; dark-mode fallback on Windows 10.
            for (attribute, value) in [
                (20, u32::from(dark)),
                (33, 1), // Disable DWM's larger automatic radius; use our 2px region.
                (34, colorref(background)),
                (35, colorref(background)),
                (36, colorref(text)),
            ] {
                unsafe {
                    DwmSetWindowAttribute(
                        handle.hwnd.get() as Hwnd,
                        attribute,
                        (&value as *const u32).cast(),
                        4,
                    );
                }
            }
        }
    }
}
