//! Windows shell calls with no safe wrapper, kept here so every other crate can forbid `unsafe`.
//! Each function checks its inputs, owns what it creates, and fails quietly (logged) rather than
//! taking the app down: none of this is essential to reading mail.
//!
//! On other systems these do nothing.

#![cfg_attr(not(windows), forbid(unsafe_code))]

/// Show `png` (a small square icon) over the app's taskbar button, or clear it with `None`, as
/// Outlook does with its envelope. `hwnd` is the main window's handle. Must be called on the
/// thread that owns the window.
pub fn set_overlay(hwnd: isize, png: Option<&[u8]>, description: &str) {
    #[cfg(windows)]
    if let Err(e) = imp::set_overlay(hwnd, png, description) {
        log::warn!("taskbar overlay: {e}");
    }
    #[cfg(not(windows))]
    let _ = (hwnd, png, description);
}

/// Play a WAV file without waiting for it to finish.
pub fn play_wav(path: &std::path::Path) -> bool {
    #[cfg(windows)]
    {
        imp::play_wav(path)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        false
    }
}

/// Set a string value under HKEY_CURRENT_USER.
pub fn set_user_registry_string(subkey: &str, name: &str, value: &str) -> bool {
    #[cfg(windows)]
    {
        imp::set_string(subkey, name, value)
    }
    #[cfg(not(windows))]
    {
        let _ = (subkey, name, value);
        false
    }
}

#[cfg(windows)]
mod imp {
    use std::cell::RefCell;
    use std::os::windows::ffi::OsStrExt as _;

    use windows::Win32::Foundation::{ERROR_SUCCESS, HWND};
    use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_FILENAME, SND_NODEFAULT};
    use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx};
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, REG_SZ, RegSetKeyValueW};
    use windows::Win32::UI::Shell::{ITaskbarList3, TaskbarList};
    use windows::Win32::UI::WindowsAndMessaging::{CreateIconFromResourceEx, DestroyIcon, HICON, LR_DEFAULTCOLOR};
    use windows::core::PCWSTR;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    struct Overlay {
        taskbar: ITaskbarList3,
        icon: Option<HICON>,
    }

    impl Drop for Overlay {
        fn drop(&mut self) {
            if let Some(i) = self.icon.take() {
                // SAFETY: an icon this module created and still owns.
                let _ = unsafe { DestroyIcon(i) };
            }
        }
    }

    thread_local! {
        static OVERLAY: RefCell<Option<Overlay>> = const { RefCell::new(None) };
    }

    pub fn set_overlay(hwnd: isize, png: Option<&[u8]>, description: &str) -> windows::core::Result<()> {
        if hwnd == 0 {
            return Ok(());
        }
        OVERLAY.with(|cell| {
            let mut slot = cell.borrow_mut();
            if slot.is_none() {
                // SAFETY: plain COM set-up on this (the window's) thread; a second initialisation
                // returns S_FALSE or RPC_E_CHANGED_MODE, both fine to ignore.
                let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
                // SAFETY: creating the shell's taskbar object and initialising it, as documented.
                let taskbar: ITaskbarList3 = unsafe { CoCreateInstance(&TaskbarList, None, CLSCTX_INPROC_SERVER)? };
                unsafe { taskbar.HrInit()? };
                *slot = Some(Overlay { taskbar, icon: None });
            }
            let Some(o) = slot.as_mut() else { return Ok(()) };
            let icon = match png {
                // SAFETY: `p` is a complete PNG in memory; Windows copies what it needs.
                Some(p) => Some(unsafe { CreateIconFromResourceEx(p, true, 0x0003_0000, 32, 32, LR_DEFAULTCOLOR)? }),
                None => None,
            };
            let text = wide(description);
            let hwnd = HWND(hwnd as *mut core::ffi::c_void);
            // SAFETY: `hwnd` is the live main window (passed in on its own thread), `icon` is ours
            // or null, and `text` outlives the call.
            unsafe { o.taskbar.SetOverlayIcon(hwnd, icon.unwrap_or_default(), PCWSTR(text.as_ptr()))? };
            // The taskbar keeps its own copy: the previous icon can go now.
            if let Some(old) = std::mem::replace(&mut o.icon, icon) {
                // SAFETY: an icon this module created, no longer in use.
                let _ = unsafe { DestroyIcon(old) };
            }
            Ok(())
        })
    }

    pub fn play_wav(path: &std::path::Path) -> bool {
        let w: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        // SAFETY: a NUL-terminated path that lives for the call; with SND_ASYNC Windows copies it.
        unsafe { PlaySoundW(PCWSTR(w.as_ptr()), None, SND_FILENAME | SND_ASYNC | SND_NODEFAULT) }.as_bool()
    }

    pub fn set_string(subkey: &str, name: &str, value: &str) -> bool {
        let (k, n, v) = (wide(subkey), wide(name), wide(value));
        // SAFETY: NUL-terminated strings that live for the call; the size is in bytes,
        // including the terminator, as REG_SZ requires.
        let r = unsafe {
            RegSetKeyValueW(HKEY_CURRENT_USER, PCWSTR(k.as_ptr()), PCWSTR(n.as_ptr()), REG_SZ.0, Some(v.as_ptr().cast()), (v.len() * 2) as u32)
        };
        r == ERROR_SUCCESS
    }
}
