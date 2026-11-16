//! Estado da janela nativa.

use gpui_kit::Window;

#[cfg(windows)]
pub fn minimizada(window: &Window) -> bool {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match HasWindowHandle::window_handle(window).map(|h| h.as_raw()) {
        // SAFETY: o HWND é da janela viva que o GPUI acabou de entregar.
        Ok(RawWindowHandle::Win32(h)) => unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::IsIconic(h.hwnd.get() as _) != 0
        },
        _ => false,
    }
}

#[cfg(not(windows))]
pub fn minimizada(_: &Window) -> bool {
    false
}
