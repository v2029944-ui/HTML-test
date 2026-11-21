"""Resize the disposable installed window using Win32, in WebView CSS pixels."""
import ctypes
from ctypes import wintypes


def set_client_width(page, app, width):
    user32 = app.user32
    signatures = {
        'GetWindowRect': (wintypes.BOOL, wintypes.HWND, ctypes.POINTER(wintypes.RECT)),
        'GetClientRect': (wintypes.BOOL, wintypes.HWND, ctypes.POINTER(wintypes.RECT)),
        'SetWindowPos': (wintypes.BOOL, wintypes.HWND, wintypes.HWND, ctypes.c_int, ctypes.c_int,
                         ctypes.c_int, ctypes.c_int, wintypes.UINT),
    }
    for name, (result, *arguments) in signatures.items():
        function = getattr(user32, name)
        function.restype, function.argtypes = result, arguments

    windows = [hwnd for hwnd in app.windows() if app.process(hwnd) == app.pid and
               app.window_class(hwnd) == 'Tauri Window' and
               user32.IsWindowVisible(hwnd) and not user32.GetWindow(hwnd, 4)]
    assert len(windows) == 1, ('Expected one visible, unowned test application window', app.pid, windows)
    hwnd = windows[0]
    client, bounds = wintypes.RECT(), wintypes.RECT()
    if not (user32.GetClientRect(hwnd, ctypes.byref(client)) and user32.GetWindowRect(hwnd, ctypes.byref(bounds))):
        raise ctypes.WinError(ctypes.get_last_error())
    # Win32 coordinates and CSS pixels can differ with the runner's display DPI.
    before = page.evaluate('innerWidth')
    assert before > 0 and client.right > 0
    frame = bounds.right - bounds.left - client.right
    desired = frame + round(width * client.right / before)
    # SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE
    if not user32.SetWindowPos(hwnd, None, 0, 0, desired, bounds.bottom - bounds.top, 0x16):
        raise ctypes.WinError(ctypes.get_last_error())
    page.wait_for_function('width => Math.abs(innerWidth - width) <= 2', arg=width, timeout=10_000)
    return page.evaluate('({width:innerWidth,height:innerHeight,devicePixelRatio})')
