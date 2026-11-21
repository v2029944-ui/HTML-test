use std::{io, ptr};
use windows_sys::Win32::{
    Foundation::HWND,
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        Shell::SetCurrentProcessExplicitAppUserModelID,
        WindowsAndMessaging::{
            LoadImageW, SendMessageW, ICON_BIG, IMAGE_ICON, LR_DEFAULTSIZE, LR_SHARED, WM_SETICON,
        },
    },
};

// NSIS stores this bundle identifier on the installed shortcuts.
pub fn set_process_identity(identifier: &str) -> io::Result<()> {
    let wide: Vec<u16> = identifier.encode_utf16().chain(Some(0)).collect();
    let result = unsafe { SetCurrentProcessExplicitAppUserModelID(wide.as_ptr()) };
    if result < 0 {
        return Err(io::Error::from_raw_os_error(result));
    }
    Ok(())
}

// Tao sets ICON_SMALL from Tauri's configured window icon. Explorer independently requests ICON_BIG.
pub fn set_large_icon(hwnd: HWND) -> io::Result<()> {
    let module = unsafe { GetModuleHandleW(ptr::null()) };
    if module.is_null() {
        return Err(io::Error::last_os_error());
    }
    let resource = tauri::utils::platform::WINDOWS_APP_ICON_RESOURCE_ID as usize as *const u16;
    // LR_SHARED keeps the module-owned HICON valid throughout the window's lifetime.
    let icon = unsafe {
        LoadImageW(
            module,
            resource,
            IMAGE_ICON,
            0,
            0,
            LR_DEFAULTSIZE | LR_SHARED,
        )
    };
    if icon.is_null() {
        return Err(io::Error::last_os_error());
    }
    unsafe { SendMessageW(hwnd, WM_SETICON, ICON_BIG as usize, icon as isize) };
    Ok(())
}
