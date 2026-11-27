use windows::{
    core::Result,
    Win32::{
        Foundation::HWND,
        Graphics::Dwm::{
            DwmSetWindowAttribute, DWMWA_CAPTION_COLOR, DWMWA_COLOR_DEFAULT, DWMWA_TEXT_COLOR,
            DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWINDOWATTRIBUTE,
        },
    },
};

fn set_attribute(hwnd: HWND, attribute: DWMWINDOWATTRIBUTE, value: u32) -> Result<()> {
    // DWM consumes a COLORREF or BOOL immediately, both four bytes wide.
    unsafe { DwmSetWindowAttribute(hwnd, attribute, (&value as *const u32).cast(), 4) }
}

pub fn apply(hwnd: HWND, background: Option<[u8; 3]>, dark: bool) -> Result<bool> {
    let caption = background.map_or(DWMWA_COLOR_DEFAULT, |[r, g, b]| {
        u32::from_le_bytes([r, g, b, 0])
    });
    // The color attributes require Windows 11. Leave the system frame alone
    // on builds that do not expose them.
    if set_attribute(hwnd, DWMWA_CAPTION_COLOR, caption).is_err() {
        return Ok(false);
    }
    let text = match background {
        None => DWMWA_COLOR_DEFAULT,
        Some(_) if dark => 0x00ff_ffff,
        Some(_) => 0x0020_2020,
    };
    set_attribute(hwnd, DWMWA_TEXT_COLOR, text)?;
    set_attribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, u32::from(dark))?;
    Ok(true)
}
