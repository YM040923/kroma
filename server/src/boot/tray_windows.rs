//! A notification-area icon for a portable Windows build, so the server runs
//! without a console window and the owner still has a way to reach it: open the
//! browser, or stop it and let it shut down cleanly.
//!
//! Everything here is Win32 through `windows-sys`, on dependencies the rest of
//! the binary already links. One thread owns the icon and its message pump; the
//! server's runtime is untouched.

use std::ffi::OsString;
use std::os::windows::ffi::OsStrExt;
use std::sync::mpsc;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Shell::{
    ShellExecuteW, Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE,
    NOTIFYICONDATAW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DispatchMessageW,
    GetCursorPos, GetMessageW, GetWindowLongPtrW, LoadIconW, PostMessageW, PostQuitMessage,
    RegisterClassW, SetForegroundWindow, SetWindowLongPtrW, TrackPopupMenu, TranslateMessage,
    GWLP_USERDATA, HMENU, MF_SEPARATOR, MF_STRING, MSG, SW_SHOWNORMAL, TPM_BOTTOMALIGN,
    TPM_LEFTALIGN, TPM_RIGHTBUTTON, WM_APP, WM_COMMAND, WM_DESTROY, WM_LBUTTONUP, WM_RBUTTONUP,
    WNDCLASSW, WS_OVERLAPPED,
};

const WM_TRAY: u32 = WM_APP + 1;
const ID_OPEN: usize = 1;
const ID_STOP: usize = 2;
const TRAY_UID: u32 = 1;

/// What the icon does when the owner uses it. Lives inside the window's
/// `GWLP_USERDATA` for the window's whole life; the window procedure on the
/// tray thread is the only reader.
struct Callbacks {
    open: Box<dyn Fn() + Send>,
    stop: Box<dyn Fn() + Send>,
}

/// Keeps the icon alive for the process's life. Dropping it closes the window,
/// which ends the tray thread and removes the icon.
pub struct TrayHandle {
    hwnd: WindowHandle,
}

/// A window handle travelling to the thread that holds the tray. The only thing
/// done with it there is `PostMessageW(WM_DESTROY)`, which Windows documents as
/// safe from any thread; the window itself is torn down by its own thread.
struct WindowHandle(HWND);
// SAFETY: see above — the handle is only used to post a message.
unsafe impl Send for WindowHandle {}

impl Drop for TrayHandle {
    fn drop(&mut self) {
        // SAFETY: the window belongs to this process's tray thread; posting a
        // destroy lets that thread run its own cleanup.
        unsafe {
            PostMessageW(self.hwnd.0, WM_DESTROY, 0, 0);
        }
    }
}

/// Starts the icon on its own thread. `open` runs when the owner clicks it;
/// `stop` when they pick "Stop".
///
/// `None` when no icon could be created (a service, a session with no shell), in
/// which case the server keeps running headless exactly as it did before there
/// was a tray.
pub fn start(
    open: impl Fn() + Send + 'static,
    stop: impl Fn() + Send + 'static,
) -> Option<TrayHandle> {
    let (ready_tx, ready_rx) = mpsc::channel::<WindowHandle>();
    std::thread::Builder::new()
        .name("kroma-tray".into())
        .spawn(move || run_icon_loop(Box::new(open), Box::new(stop), &ready_tx))
        .ok()?;
    let hwnd = ready_rx.recv_timeout(std::time::Duration::from_secs(5)).ok()?;
    Some(TrayHandle { hwnd })
}

fn run_icon_loop(
    open: Box<dyn Fn() + Send>,
    stop: Box<dyn Fn() + Send>,
    ready: &mpsc::Sender<WindowHandle>,
) {
    let class = wide("KROMA.Tray");
    let title = wide("KROMA");
    // SAFETY: the module handle names this process's own image.
    let instance = unsafe { GetModuleHandleW(std::ptr::null()) };

    let wnd_class = WNDCLASSW {
        style: 0,
        lpfnWndProc: Some(window_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: instance,
        hIcon: std::ptr::null_mut(),
        hCursor: std::ptr::null_mut(),
        hbrBackground: std::ptr::null_mut(),
        lpszMenuName: std::ptr::null(),
        lpszClassName: class.as_ptr(),
    };
    // SAFETY: `wnd_class` outlives the call and names strings we own.
    if unsafe { RegisterClassW(&wnd_class) } == 0 {
        return;
    }
    // A hidden window: never shown, only the target for the icon's callbacks.
    // SAFETY: the class is registered above and every pointer is a local.
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        )
    };
    if hwnd.is_null() {
        return;
    }
    // The callbacks live as long as the window: the box is leaked here and
    // reclaimed on WM_NCDESTROY, so the procedure can reach them by handle.
    let callbacks = Box::into_raw(Box::new(Callbacks { open, stop }));
    // SAFETY: `hwnd` was just created on this thread.
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, callbacks as isize);
    }

    let icon = load_app_icon(instance);
    if !add_icon(hwnd, icon) {
        // SAFETY: `hwnd` was just created on this thread.
        unsafe { PostMessageW(hwnd, WM_DESTROY, 0, 0) };
        return;
    }
    let _ = ready.send(WindowHandle(hwnd));

    let mut msg: MSG = unsafe { std::mem::zeroed() };
    loop {
        // SAFETY: `msg` is a local the call fills in.
        let got = unsafe { GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) };
        if got <= 0 {
            break;
        }
        // Every notification and menu command is routed through the window
        // procedure below, which is where the icon's callbacks belong: the shell
        // posts WM_TRAY to the window, not to this loop.
        // SAFETY: `msg` is the one GetMessage filled.
        unsafe {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    remove_icon(hwnd);
    // SAFETY: the window is gone; reclaim the callbacks it held.
    unsafe {
        let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Callbacks;
        if !ptr.is_null() {
            drop(Box::from_raw(ptr));
        }
    }
}

fn show_menu(hwnd: HWND) {
    let open = wide("Open KROMA");
    let stop = wide("Stop KROMA");
    // SAFETY: the menu is created and destroyed here; the strings outlive use.
    unsafe {
        let menu: HMENU = CreatePopupMenu();
        if menu.is_null() {
            return;
        }
        AppendMenuW(menu, MF_STRING, ID_OPEN, open.as_ptr());
        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
        AppendMenuW(menu, MF_STRING, ID_STOP, stop.as_ptr());
        let mut point = POINT { x: 0, y: 0 };
        GetCursorPos(&mut point);
        // A popup with no foreground owner dismisses the moment it opens; the
        // hidden tray window must be foreground before it can hold the menu.
        SetForegroundWindow(hwnd);
        TrackPopupMenu(
            menu,
            TPM_LEFTALIGN | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN,
            point.x,
            point.y,
            0,
            hwnd,
            std::ptr::null(),
        );
        DestroyMenu(menu);
    }
}

/// Hands a URL to the shell's default handler. `ShellExecuteW` is the right
/// call from a GUI process: it opens the browser with no console and no window
/// bounce, unlike a `cmd /c start` shim.
fn open_url(url: &str) {
    let target = wide(url);
    // SAFETY: the string is a NUL-terminated local that outlives the call; the
    // verb is left null so the shell applies the default ("open") action.
    unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            std::ptr::null(),
            target.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        );
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: the pointer was stored for this window's lifetime on this thread.
    let callbacks = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Callbacks };
    match message {
        WM_TRAY => {
            match lparam as u32 {
                WM_LBUTTONUP => run_open(callbacks),
                WM_RBUTTONUP => show_menu(hwnd),
                _ => {}
            }
            0
        }
        WM_COMMAND => {
            match wparam {
                ID_OPEN => run_open(callbacks),
                ID_STOP => run_stop(callbacks),
                _ => {}
            }
            0
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

fn run_open(callbacks: *mut Callbacks) {
    // SAFETY: set for the window's life, cleared only after the loop ends.
    if let Some(cb) = unsafe { callbacks.as_ref() } {
        (cb.open)();
    }
}

fn run_stop(callbacks: *mut Callbacks) {
    // SAFETY: as in `run_open`.
    if let Some(cb) = unsafe { callbacks.as_ref() } {
        (cb.stop)();
    }
}

fn load_app_icon(instance: *mut std::ffi::c_void) -> *mut std::ffi::c_void {
    // The icon build.rs compiled into the exe sits at resource id 1.
    const ICON_RESOURCE_ID: usize = 1;
    // SAFETY: reading this process's own icon resource.
    unsafe { LoadIconW(instance, ICON_RESOURCE_ID as *const u16) }
}

fn add_icon(hwnd: HWND, icon: *mut std::ffi::c_void) -> bool {
    let mut data: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
    data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    data.hWnd = hwnd;
    data.uID = TRAY_UID;
    data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    data.uCallbackMessage = WM_TRAY;
    data.hIcon = icon;
    let tip = wide("KROMA");
    for (slot, ch) in data.szTip.iter_mut().zip(tip.iter()) {
        *slot = *ch;
    }
    // SAFETY: `data` is fully initialised for the call.
    unsafe { Shell_NotifyIconW(NIM_ADD, &data) != 0 }
}

fn remove_icon(hwnd: HWND) {
    let mut data: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
    data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    data.hWnd = hwnd;
    data.uID = TRAY_UID;
    // SAFETY: as in `add_icon`.
    unsafe {
        Shell_NotifyIconW(NIM_DELETE, &data);
    }
}

fn wide(text: &str) -> Vec<u16> {
    OsString::from(text)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// Opens `url` in the default browser. Public so the server can reuse it for
/// the "open on start" path without duplicating the Win32 call.
pub fn open_in_browser(url: &str) {
    open_url(url);
}
