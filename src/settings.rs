//! The few settings that survive a restart, kept in the Windows registry under
//! HKEY_CURRENT_USER (no admin rights, no config file to find).
//!
//! On other platforms nothing is saved: every launch starts from the defaults.

/// Snap values instead of animating them: about one frame per reading.
pub fn reduce_motion() -> bool {
    imp::reduce_motion()
}

pub fn set_reduce_motion(on: bool) {
    imp::set_reduce_motion(on);
}

#[cfg(windows)]
pub use imp::{autostart, set_autostart};

#[cfg(windows)]
mod imp {
    use std::ptr::null_mut;
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_DWORD, REG_SZ, RRF_RT_REG_DWORD,
        RRF_RT_REG_SZ,
    };

    const KEY: &str = r"Software\Vitals";
    // The per-user list of programs Windows starts at sign-in. Entries here show up
    // in Task Manager's "Startup apps", where they can be disabled too.
    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const RUN_VALUE: &str = "Vitals";

    /// UTF-16 with the terminating NUL the registry API expects.
    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain([0]).collect()
    }

    pub fn reduce_motion() -> bool {
        let mut data = 0u32;
        let mut size = size_of::<u32>() as u32;
        let ok = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                wide(KEY).as_ptr(),
                wide("ReduceMotion").as_ptr(),
                RRF_RT_REG_DWORD,
                null_mut(),
                (&raw mut data).cast(),
                &mut size,
            )
        } == ERROR_SUCCESS;
        ok && data != 0
    }

    pub fn set_reduce_motion(on: bool) {
        let data = on as u32;
        // Creates the key if needed. Failing to save isn't worth bothering anyone about.
        unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                wide(KEY).as_ptr(),
                wide("ReduceMotion").as_ptr(),
                REG_DWORD,
                (&raw const data).cast(),
                size_of::<u32>() as u32,
            );
        }
    }

    /// Whether Windows will start Vitals at sign-in.
    pub fn autostart() -> bool {
        unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                wide(RUN_KEY).as_ptr(),
                wide(RUN_VALUE).as_ptr(),
                RRF_RT_REG_SZ,
                null_mut(),
                null_mut(),
                null_mut(),
            ) == ERROR_SUCCESS
        }
    }

    /// Registers (or removes) this exe, wherever it is now, to start hidden in the tray at sign-in.
    pub fn set_autostart(on: bool) {
        let (key, value) = (wide(RUN_KEY), wide(RUN_VALUE));
        if !on {
            unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), value.as_ptr()) };
            return;
        }
        let Ok(exe) = std::env::current_exe() else { return };
        let command = wide(&format!("\"{}\" --hidden", exe.display()));
        unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                value.as_ptr(),
                REG_SZ,
                command.as_ptr().cast(),
                (command.len() * size_of::<u16>()) as u32,
            );
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn reduce_motion() -> bool {
        false
    }

    pub fn set_reduce_motion(_on: bool) {}
}
