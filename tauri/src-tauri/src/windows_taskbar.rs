use std::{ffi::OsString, os::windows::ffi::OsStrExt};
use windows::{
    core::{Result, BSTR, HSTRING, PCWSTR},
    Win32::{
        Foundation::{HWND, LPARAM, WPARAM},
        Storage::EnhancedStorage::{
            PKEY_AppUserModel_ID, PKEY_AppUserModel_RelaunchCommand,
            PKEY_AppUserModel_RelaunchDisplayNameResource, PKEY_AppUserModel_RelaunchIconResource,
        },
        System::{
            Com::StructuredStorage::{PropVariantChangeType, PROPVARIANT, PVCHF_DEFAULT},
            LibraryLoader::GetModuleHandleW,
            Variant::VT_LPWSTR,
        },
        UI::{
            Shell::{
                PropertiesSystem::{IPropertyStore, SHGetPropertyStoreForWindow},
                SetCurrentProcessExplicitAppUserModelID,
            },
            WindowsAndMessaging::{
                LoadImageW, SendMessageW, ICON_BIG, IMAGE_ICON, LR_DEFAULTSIZE, LR_SHARED,
                WM_SETICON,
            },
        },
    },
};

pub fn set_process_identity(identifier: &str) -> Result<()> {
    unsafe { SetCurrentProcessExplicitAppUserModelID(&HSTRING::from(identifier)) }
}

// Tao sets ICON_SMALL from the configured window icon. The module owns ICON_BIG.
pub fn set_large_icon(hwnd: HWND) -> Result<()> {
    let resource = tauri::utils::platform::WINDOWS_APP_ICON_RESOURCE_ID;
    unsafe {
        let module = GetModuleHandleW(None)?;
        let icon = LoadImageW(
            Some(module.into()),
            PCWSTR(resource as usize as *const u16),
            IMAGE_ICON,
            0,
            0,
            LR_DEFAULTSIZE | LR_SHARED,
        )?;
        SendMessageW(
            hwnd,
            WM_SETICON,
            Some(WPARAM(ICON_BIG as usize)),
            Some(LPARAM(icon.0 as isize)),
        );
    }
    Ok(())
}

fn string_property(value: OsString) -> Result<PROPVARIANT> {
    let wide: Vec<u16> = value.encode_wide().collect();
    let source = PROPVARIANT::from(BSTR::from_wide(&wide));
    let mut result = PROPVARIANT::default();
    unsafe {
        PropVariantChangeType(&mut result, &source, PVCHF_DEFAULT, VT_LPWSTR)?;
    }
    Ok(result)
}

pub fn configure_relaunch(hwnd: HWND, identifier: &str) -> Result<()> {
    let exe = std::env::current_exe()?;
    let mut command = OsString::from("\"");
    command.push(&exe);
    command.push("\"");
    let mut display = OsString::from("@");
    display.push(&exe);
    display.push(",-101"); // STRINGTABLE in build.rs
    let ico = exe.with_file_name("analysis-bars-blue.ico");
    let icon = if ico.is_file() {
        let mut path = ico.into_os_string();
        path.push(",0");
        path
    } else {
        let mut path = exe.into_os_string();
        path.push(format!(
            ",-{}",
            tauri::utils::platform::WINDOWS_APP_ICON_RESOURCE_ID
        ));
        path
    };
    let store: IPropertyStore = unsafe { SHGetPropertyStoreForWindow(hwnd)? };
    unsafe {
        store.SetValue(
            &PKEY_AppUserModel_RelaunchCommand,
            &string_property(command)?,
        )?;
        store.SetValue(
            &PKEY_AppUserModel_RelaunchDisplayNameResource,
            &string_property(display)?,
        )?;
        store.SetValue(
            &PKEY_AppUserModel_RelaunchIconResource,
            &string_property(icon)?,
        )?;
        // Setting the ID last lets Explorer read all relaunch properties when grouping.
        store.SetValue(
            &PKEY_AppUserModel_ID,
            &string_property(OsString::from(identifier))?,
        )?;
        store.Commit()
    }
}

pub fn clear_relaunch(hwnd: HWND) -> Result<()> {
    let store: IPropertyStore = unsafe { SHGetPropertyStoreForWindow(hwnd)? };
    let empty = PROPVARIANT::default();
    unsafe {
        for key in [
            PKEY_AppUserModel_ID,
            PKEY_AppUserModel_RelaunchIconResource,
            PKEY_AppUserModel_RelaunchDisplayNameResource,
            PKEY_AppUserModel_RelaunchCommand,
        ] {
            store.SetValue(&key, &empty)?;
        }
        store.Commit()
    }
}
