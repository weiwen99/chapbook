use std::cell::Cell;
use std::io;
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{
    GetLastError, HINSTANCE, HWND, LPARAM, LRESULT, RECT, SetLastError, WPARAM,
};
use windows_sys::Win32::Graphics::Gdi::{
    COLOR_BTNFACE, DEFAULT_GUI_FONT, GetStockObject, HGDIOBJ, UpdateWindow,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::*;
use windows_sys::core::{PCWSTR, w};

const CLASS_NAME: PCWSTR = w!("ChapbookLauncherWindow");
const WINDOW_TITLE: PCWSTR = w!("chapbook — 本地阅读服务");
const SERVER_STOPPED: u32 = WM_APP + 1;
const FOLDER_ID: u16 = 1001;
const URL_ID: u16 = 1002;
const OPEN_BROWSER_ID: u16 = 1003;
const STOP_ID: u16 = 1004;

pub fn pick_folder() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title("选择要阅读的文件夹")
        .pick_folder()
}

pub fn show_error(message: &str) {
    let message = wide(message);
    // MessageBox copies/uses this UTF-16 buffer synchronously; there is no owner
    // window before initialization succeeds (or after the status window closes).
    unsafe {
        MessageBoxW(
            null_mut(),
            message.as_ptr(),
            w!("chapbook — 错误"),
            MB_OK | MB_ICONERROR | MB_TASKMODAL,
        );
    }
}

#[derive(Clone, Copy)]
pub enum Event {
    OpenBrowser,
    Stop,
    ServerStopped,
}

struct WindowState {
    hwnd: Cell<HWND>,
    event: Cell<Option<Event>>,
    initialized: Cell<bool>,
    initialization_error: Cell<Option<u32>>,
}

pub struct StatusWindow {
    // The allocation remains stable even when StatusWindow moves. GWLP_USERDATA
    // borrows it only until WM_NCDESTROY clears the pointer. Cell permits Win32
    // callback reentrancy without creating overlapping mutable references.
    state: Box<WindowState>,
    _class: WindowClass,
}

#[derive(Clone, Copy)]
pub struct ServerNotifier {
    // No Rust pointer crosses threads or enters the message queue. An HWND is
    // only an OS token; the notifier neither owns nor dereferences the window.
    hwnd: usize,
}

impl ServerNotifier {
    pub fn notify(self) {
        if self.hwnd != 0 {
            // PostMessage is thread-safe and carries no borrowed data. A late
            // notification after window destruction is harmless and may fail;
            // the owning thread is already stopping in that case.
            unsafe {
                PostMessageW(self.hwnd as HWND, SERVER_STOPPED, 0, 0);
            }
        }
    }
}

impl StatusWindow {
    pub fn new(root: &Path, url: &str) -> io::Result<Self> {
        let class = WindowClass::new()?;
        let window = Self {
            state: Box::new(WindowState {
                hwnd: Cell::new(null_mut()),
                event: Cell::new(None),
                initialized: Cell::new(false),
                initialization_error: Cell::new(None),
            }),
            _class: class,
        };
        let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
        let ex_style = WS_EX_APPWINDOW | WS_EX_CONTROLPARENT;
        let mut bounds = RECT {
            left: 0,
            top: 0,
            right: 700,
            bottom: 290,
        };
        // All pointers below refer to live local buffers or to the stable state
        // allocation. CreateWindowExW sends WM_NCCREATE synchronously.
        unsafe {
            if AdjustWindowRectEx(&mut bounds, style, 0, ex_style) == 0 {
                return Err(win32_error("AdjustWindowRectEx"));
            }
            let hwnd = CreateWindowExW(
                ex_style,
                CLASS_NAME,
                WINDOW_TITLE,
                style,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                bounds.right - bounds.left,
                bounds.bottom - bounds.top,
                null_mut(),
                null_mut(),
                window._class.instance,
                (&*window.state as *const WindowState).cast(),
            );
            if hwnd.is_null() {
                if let Some(code) = window.state.initialization_error.get() {
                    return Err(io::Error::from_raw_os_error(code as i32));
                }
                return Err(win32_error("CreateWindowExW (status window)"));
            }
            let font = GetStockObject(DEFAULT_GUI_FONT);
            if font.is_null() {
                return Err(win32_error("GetStockObject (default GUI font)"));
            }
            window.control(
                w!("STATIC"),
                "服务正在运行",
                1101,
                20,
                20,
                660,
                24,
                0,
                0,
                font,
            )?;
            window.control(
                w!("STATIC"),
                "文件夹(&F)：",
                1102,
                20,
                56,
                660,
                22,
                0,
                0,
                font,
            )?;
            window.control(
                w!("EDIT"),
                &chapbook::meta::display_path_text(dunce::simplified(root)),
                FOLDER_ID,
                20,
                80,
                660,
                28,
                WS_TABSTOP | ES_READONLY as u32 | ES_AUTOHSCROLL as u32,
                WS_EX_CLIENTEDGE,
                font,
            )?;
            window.control(
                w!("STATIC"),
                "访问地址(&U)：",
                1103,
                20,
                122,
                660,
                22,
                0,
                0,
                font,
            )?;
            window.control(
                w!("EDIT"),
                url,
                URL_ID,
                20,
                146,
                660,
                28,
                WS_TABSTOP | ES_READONLY as u32 | ES_AUTOHSCROLL as u32,
                WS_EX_CLIENTEDGE,
                font,
            )?;
            window.control(
                w!("STATIC"),
                "仅本机可访问。关闭此窗口将停止服务并退出。",
                1104,
                20,
                190,
                660,
                24,
                0,
                0,
                font,
            )?;
            window.control(
                w!("BUTTON"),
                "重新打开浏览器(&O)",
                OPEN_BROWSER_ID,
                304,
                234,
                190,
                34,
                WS_TABSTOP | BS_DEFPUSHBUTTON as u32,
                0,
                font,
            )?;
            window.control(
                w!("BUTTON"),
                "停止并退出(&X)",
                STOP_ID,
                510,
                234,
                170,
                34,
                WS_TABSTOP | BS_PUSHBUTTON as u32,
                0,
                font,
            )?;
            window.state.initialized.set(true);
            // ShowWindow returns the *previous* visibility, not a success flag.
            ShowWindow(hwnd, SW_SHOWNORMAL);
            if UpdateWindow(hwnd) == 0 {
                return Err(win32_error("UpdateWindow"));
            }
        }
        Ok(window)
    }

    pub fn notifier(&self) -> ServerNotifier {
        ServerNotifier {
            hwnd: self.state.hwnd.get() as usize,
        }
    }

    pub fn next_event(&self) -> io::Result<Event> {
        loop {
            if let Some(event) = self.state.event.take() {
                return Ok(event);
            }
            let hwnd = self.state.hwnd.get();
            if hwnd.is_null() {
                return Ok(Event::Stop);
            }
            // GetMessage writes into this local MSG. DispatchMessage invokes
            // the callback on this same GUI thread; state outlives every call.
            unsafe {
                let mut message = std::mem::zeroed::<MSG>();
                let result = GetMessageW(&mut message, null_mut(), 0, 0);
                if result == -1 {
                    return Err(win32_error("GetMessageW"));
                }
                if result == 0 {
                    return Ok(Event::Stop);
                }
                // Standard tab/mnemonic/default-button keyboard navigation for
                // the ordinary top-level window and its conventional controls.
                if IsDialogMessageW(hwnd, &message) == 0 {
                    TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn control(
        &self,
        class: PCWSTR,
        text: &str,
        id: u16,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        style: u32,
        ex_style: u32,
        font: HGDIOBJ,
    ) -> io::Result<()> {
        let text = wide(text);
        // The parent owns all child HWNDs, including on a later initialization
        // failure. Text is copied during CreateWindowExW; the stock font is
        // borrowed for the process lifetime and must not be deleted.
        unsafe {
            let child = CreateWindowExW(
                ex_style,
                class,
                text.as_ptr(),
                WS_CHILD | WS_VISIBLE | style,
                x,
                y,
                width,
                height,
                self.state.hwnd.get(),
                id as usize as HMENU,
                self._class.instance,
                null(),
            );
            if child.is_null() {
                return Err(win32_error("CreateWindowExW (control)"));
            }
            SendMessageW(child, WM_SETFONT, font as usize, 1);
        }
        Ok(())
    }
}

impl Drop for StatusWindow {
    fn drop(&mut self) {
        let hwnd = self.state.hwnd.get();
        if !hwnd.is_null() {
            // WM_NCDESTROY clears the stored HWND, so this cannot destroy a
            // recycled handle after the user has already closed our window.
            // Raw HWND fields also keep StatusWindow on its owning GUI thread.
            unsafe {
                DestroyWindow(hwnd);
            }
        }
    }
}

struct WindowClass {
    // GetModuleHandle borrows the executable module; never FreeLibrary it.
    instance: HINSTANCE,
}

impl WindowClass {
    fn new() -> io::Result<Self> {
        // These standard icon/cursor handles are shared system resources, not
        // owned objects. The class registration itself is owned and released.
        unsafe {
            let instance = GetModuleHandleW(null());
            if instance.is_null() {
                return Err(win32_error("GetModuleHandleW"));
            }
            let icon = LoadIconW(null_mut(), IDI_APPLICATION);
            if icon.is_null() {
                return Err(win32_error("LoadIconW"));
            }
            let cursor = LoadCursorW(null_mut(), IDC_ARROW);
            if cursor.is_null() {
                return Err(win32_error("LoadCursorW"));
            }
            let class = WNDCLASSW {
                lpfnWndProc: Some(window_proc),
                hInstance: instance,
                hIcon: icon,
                hCursor: cursor,
                hbrBackground: (COLOR_BTNFACE + 1) as usize as _,
                lpszClassName: CLASS_NAME,
                ..std::mem::zeroed()
            };
            if RegisterClassW(&class) == 0 {
                return Err(win32_error("RegisterClassW"));
            }
            Ok(Self { instance })
        }
    }
}

impl Drop for WindowClass {
    fn drop(&mut self) {
        // StatusWindow's Drop has destroyed the window and all its children
        // before this field is dropped; no callback can use the class now.
        unsafe {
            UnregisterClassW(CLASS_NAME, self.instance);
        }
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // Win32 supplies a valid CREATESTRUCTW only for WM_NCCREATE. Thereafter
    // userdata is either null or our stable borrowed WindowState allocation.
    unsafe {
        if message == WM_NCCREATE {
            let create = &*(lparam as *const CREATESTRUCTW);
            let state = &*(create.lpCreateParams as *const WindowState);
            SetLastError(0);
            let previous = SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
            if previous == 0 {
                let code = GetLastError();
                if code != 0 {
                    state.initialization_error.set(Some(code));
                    return 0;
                }
            }
            state.hwnd.set(hwnd);
            // Default non-client initialization stores the window caption.
            return DefWindowProcW(hwnd, message, wparam, lparam);
        }
        let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const WindowState;
        if !state.is_null() {
            let state = &*state;
            match message {
                WM_COMMAND if ((wparam >> 16) & 0xffff) as u32 == BN_CLICKED => {
                    let event = match (wparam & 0xffff) as u16 {
                        OPEN_BROWSER_ID => Some(Event::OpenBrowser),
                        STOP_ID => Some(Event::Stop),
                        _ => None,
                    };
                    if let Some(event) = event {
                        state.event.set(Some(event));
                        // BM_CLICK/WM_COMMAND can arrive as synchronous sent
                        // messages while GetMessage is blocked. A posted wakeup
                        // makes it return so next_event observes the Cell too.
                        PostMessageW(hwnd, WM_NULL, 0, 0);
                        return 0;
                    }
                }
                SERVER_STOPPED => {
                    state.event.set(Some(Event::ServerStopped));
                    return 0;
                }
                WM_CLOSE => {
                    DestroyWindow(hwnd);
                    return 0;
                }
                WM_DESTROY => {
                    state.event.set(Some(Event::Stop));
                    // Do not leave WM_QUIT behind if initialization failed and
                    // its partially created window is being unwound.
                    if state.initialized.get() {
                        PostQuitMessage(0);
                    }
                    return 0;
                }
                WM_NCDESTROY => {
                    state.hwnd.set(null_mut());
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                }
                _ => {}
            }
        }
        DefWindowProcW(hwnd, message, wparam, lparam)
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn win32_error(operation: &str) -> io::Error {
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(0) {
        io::Error::other(format!("{operation} failed"))
    } else {
        io::Error::new(error.kind(), format!("{operation}: {error}"))
    }
}
