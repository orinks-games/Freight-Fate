//! SDL drops Windows keys with zero scan codes except arrows; braille notetakers and automation tools can send keys that way.

#[cfg(windows)]
use std::cell::Cell;
#[cfg(windows)]
use std::sync::OnceLock;

#[cfg(windows)]
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
#[cfg(windows)]
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    MapVirtualKeyW, VkKeyScanW, MAPVK_VK_TO_VSC_EX, VK_PACKET,
};
#[cfg(windows)]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallWindowProcW, DefWindowProcW, SetWindowLongPtrW, GWLP_WNDPROC, WM_CHAR, WM_KEYDOWN,
    WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP, WNDPROC,
};

const SCAN_CODE_MASK: u64 = 0xff << 16;
const EXTENDED_KEY_MASK: u64 = 1 << 24;

fn fill_scan_code(lparam: isize, mapped: u32) -> isize {
    let value = lparam as u64;
    if value & SCAN_CODE_MASK != 0 || mapped == 0 {
        return lparam;
    }

    let mut patched = value | (u64::from(mapped & 0xff) << 16);
    if is_extended_scan(mapped) {
        patched |= EXTENDED_KEY_MASK;
    }
    patched as isize
}

fn key_lparam(scan: u32, extended: bool, up: bool) -> isize {
    let mut value = 1 | (u64::from(scan & 0xff) << 16);
    if extended {
        value |= EXTENDED_KEY_MASK;
    }
    if up {
        value |= 0xc000_0000;
    }
    value as isize
}

fn is_extended_scan(mapped: u32) -> bool {
    matches!(mapped >> 8, 0xe0 | 0xe1)
}

#[cfg(windows)]
static PREVIOUS_WNDPROC: OnceLock<WNDPROC> = OnceLock::new();

#[cfg(windows)]
thread_local! {
    static PACKET_CHAR_PENDING: Cell<bool> = const { Cell::new(false) };
    static PACKET_KEY_TO_RELEASE: Cell<Option<(u32, u32)>> = const { Cell::new(None) };
}

#[cfg(windows)]
pub(super) fn install(hwnd: isize) {
    PREVIOUS_WNDPROC.get_or_init(|| {
        // SAFETY: hwnd came from the live SDL Win32 window; the replacement
        // procedure forwards every message to the procedure returned here.
        let previous = unsafe {
            SetWindowLongPtrW(
                hwnd as HWND,
                GWLP_WNDPROC,
                window_proc as *const () as usize as isize,
            )
        };
        if previous == 0 {
            log::warn!("keyboard: could not subclass the SDL window");
            return None;
        }
        // SAFETY: SetWindowLongPtrW returned the previous non-null WNDPROC.
        unsafe { std::mem::transmute::<isize, WNDPROC>(previous) }
    });
}

#[cfg(windows)]
fn forward(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match PREVIOUS_WNDPROC.get().copied().flatten() {
        // SAFETY: the original SDL procedure is retained for this live window.
        Some(previous) => unsafe { CallWindowProcW(Some(previous), hwnd, message, wparam, lparam) },
        // SAFETY: this is only reached if subclass installation failed.
        None => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

#[cfg(windows)]
unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let packet_key = wparam == usize::from(VK_PACKET);
    match message {
        WM_KEYDOWN | WM_SYSKEYDOWN if packet_key => {
            PACKET_CHAR_PENDING.with(|pending| pending.set(true));
            PACKET_KEY_TO_RELEASE.with(|key| key.set(None));
        }
        WM_KEYUP | WM_SYSKEYUP if packet_key => {
            PACKET_CHAR_PENDING.with(|pending| pending.set(false));
            if let Some((vk, mapped)) = PACKET_KEY_TO_RELEASE.with(Cell::take) {
                let key_up = key_lparam(mapped, is_extended_scan(mapped), true);
                let _ = forward(hwnd, WM_KEYUP, vk as WPARAM, key_up);
            }
            return forward(hwnd, message, wparam, lparam);
        }
        WM_CHAR => {
            if PACKET_CHAR_PENDING.with(|pending| pending.replace(false)) {
                synthesize_packet_key(hwnd, wparam as u16);
            }
        }
        WM_KEYDOWN | WM_KEYUP | WM_SYSKEYDOWN | WM_SYSKEYUP
            if lparam as u64 & SCAN_CODE_MASK == 0 =>
        {
            // SAFETY: MapVirtualKeyW accepts any virtual-key code and does
            // not retain or dereference the supplied value.
            let mapped = unsafe { MapVirtualKeyW(wparam as u32, MAPVK_VK_TO_VSC_EX) };
            return forward(hwnd, message, wparam, fill_scan_code(lparam, mapped));
        }
        _ => {}
    }
    forward(hwnd, message, wparam, lparam)
}

#[cfg(windows)]
fn synthesize_packet_key(hwnd: HWND, character: u16) {
    // SAFETY: VkKeyScanW maps a UTF-16 code unit using the current keyboard
    // layout and does not retain the character.
    let translated = unsafe { VkKeyScanW(character) } as u16;
    let vk = u32::from(translated & 0xff);
    if vk == 0xff {
        return;
    }

    // SAFETY: MapVirtualKeyW accepts the virtual-key value returned above.
    let mapped = unsafe { MapVirtualKeyW(vk, MAPVK_VK_TO_VSC_EX) };
    if mapped == 0 {
        return;
    }

    let key_down = key_lparam(mapped, is_extended_scan(mapped), false);
    let _ = forward(hwnd, WM_KEYDOWN, vk as WPARAM, key_down);
    PACKET_KEY_TO_RELEASE.with(|key| key.set(Some((vk, mapped))));
}

#[cfg(test)]
mod tests {
    use super::{fill_scan_code, key_lparam};

    #[test]
    fn fills_return_scan_code_without_extended_flag() {
        let patched = fill_scan_code(0, 0x1c) as u64;
        assert_eq!((patched >> 16) & 0xff, 0x1c);
        assert_eq!((patched >> 24) & 1, 0);
    }

    #[test]
    fn fills_up_arrow_scan_code_and_extended_flag() {
        let patched = fill_scan_code(0, 0xe048) as u64;
        assert_eq!((patched >> 16) & 0xff, 0x48);
        assert_eq!((patched >> 24) & 1, 1);
    }

    #[test]
    fn leaves_an_existing_scan_code_unchanged() {
        let lparam = 0x0039_0007;
        assert_eq!(fill_scan_code(lparam, 0xe048), lparam);
    }

    #[test]
    fn leaves_an_unmapped_key_unchanged() {
        let lparam = 0x1234_0007;
        assert_eq!(fill_scan_code(lparam, 0), lparam);
    }

    #[test]
    fn preserves_sender_extended_flag() {
        let lparam = 1 << 24;
        assert_eq!(
            fill_scan_code(lparam, 0x1c) as u64,
            (1 << 24) | (0x1c << 16)
        );
    }

    #[test]
    fn preserves_repeat_count_and_key_up_context_bits() {
        let lparam = 0xe000_0007u32 as isize;
        let patched = fill_scan_code(lparam, 0x1c) as u64;
        assert_eq!(patched & 0xffff, 7);
        assert_eq!(patched & 0xe000_0000, 0xe000_0000);
    }

    #[test]
    fn builds_key_down_and_key_up_lparams() {
        assert_eq!(key_lparam(0x1c, false, false), 0x001c_0001);
        assert_eq!(key_lparam(0xe048, true, true) as u64, 0xc148_0001);
    }
}
