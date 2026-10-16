use std::cell::Cell;
use std::io;
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::sync::Arc;

use parking_lot::Mutex;

use windows_sys::Win32::Foundation::{
    GetLastError, HINSTANCE, HWND, LPARAM, LRESULT, RECT, SetLastError, WPARAM,
};
use windows_sys::Win32::Graphics::Gdi::{
    COLOR_BTNFACE, DEFAULT_GUI_FONT, GetStockObject, HGDIOBJ, UpdateWindow,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
use windows_sys::Win32::UI::WindowsAndMessaging::*;
use windows_sys::core::{PCWSTR, w};

const CLASS_NAME: PCWSTR = w!("ChapbookLauncherWindow");
const WINDOW_TITLE: PCWSTR = w!("chapbook — 状态窗口");
const SERVER_STOPPED: u32 = WM_APP + 1;
const BROWSER_FINISHED: u32 = WM_APP + 2;
const STATUS_ID: u16 = 1104;
static NEXT_WINDOW_ID: Mutex<usize> = Mutex::new(1);
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

// Later variants take precedence if sent commands arrive while GetMessage is
// retrieving a queued worker notification. In particular, Stop is never lost.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Event {
    OpenBrowser,
    BrowserFinished,
    ServerStopped,
    Stop,
}

struct ControlBounds {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

struct WindowState {
    hwnd: Cell<HWND>,
    event: Cell<Option<Event>>,
    initialized: Cell<bool>,
    initialization_error: Cell<Option<u32>>,
    notification: Arc<NotificationTarget>,
}

impl WindowState {
    fn queue_event(&self, event: Event) {
        self.event.set(Some(
            self.event.get().map_or(event, |pending| pending.max(event)),
        ));
    }
}

pub struct StatusWindow {
    // The allocation remains stable even when StatusWindow moves. GWLP_USERDATA
    // borrows it only until WM_NCDESTROY clears the pointer. Cell permits Win32
    // callback reentrancy without creating overlapping mutable references.
    state: Box<WindowState>,
    _class: WindowClass,
    font: HGDIOBJ,
}

#[derive(Clone)]
pub struct Notifier {
    target: Arc<NotificationTarget>,
}

struct NotificationTarget {
    hwnd: Mutex<Option<usize>>,
    id: usize,
}

impl Notifier {
    pub fn server_stopped(&self) {
        self.notify(SERVER_STOPPED);
    }

    pub fn browser_finished(&self) {
        self.notify(BROWSER_FINISHED);
    }

    fn notify(&self, message: u32) {
        let hwnd = self.target.hwnd.lock();
        if let Some(hwnd) = *hwnd {
            // Posting is nonblocking. Destruction invalidates the target under
            // this same lock; a per-window ID also rejects already-queued
            // messages if Windows later recycles the numeric HWND.
            unsafe {
                PostMessageW(hwnd as HWND, message, self.target.id, 0);
            }
        }
    }
}

impl StatusWindow {
    pub fn new(root: &Path, url: &str) -> io::Result<Self> {
        let class = WindowClass::new()?;
        let id = {
            let mut next = NEXT_WINDOW_ID.lock();
            let id = *next;
            *next = id
                .checked_add(1)
                .ok_or_else(|| io::Error::other("status window identifier exhausted"))?;
            id
        };
        // The stock font is borrowed for the process lifetime.
        let font = unsafe { GetStockObject(DEFAULT_GUI_FONT) };
        if font.is_null() {
            return Err(win32_error("GetStockObject (default GUI font)"));
        }
        let window = Self {
            state: Box::new(WindowState {
                hwnd: Cell::new(null_mut()),
                event: Cell::new(None),
                initialized: Cell::new(false),
                initialization_error: Cell::new(None),
                notification: Arc::new(NotificationTarget {
                    hwnd: Mutex::new(None),
                    id,
                }),
            }),
            _class: class,
            font,
        };
        let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
        let ex_style = WS_EX_APPWINDOW | WS_EX_CONTROLPARENT;
        let mut bounds = RECT {
            left: 0,
            top: 0,
            right: 700,
            bottom: 380,
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
            window.control(
                w!("STATIC"),
                "服务正在运行",
                1101,
                ControlBounds {
                    x: 20,
                    y: 20,
                    width: 660,
                    height: 24,
                },
                0,
                0,
            )?;
            window.control(
                w!("STATIC"),
                "文件夹(&F)：",
                1102,
                ControlBounds {
                    x: 20,
                    y: 56,
                    width: 660,
                    height: 22,
                },
                0,
                0,
            )?;
            window.control(
                w!("EDIT"),
                &chapbook::meta::display_path_text(dunce::simplified(root)),
                FOLDER_ID,
                ControlBounds {
                    x: 20,
                    y: 80,
                    width: 660,
                    height: 28,
                },
                WS_TABSTOP | ES_READONLY as u32 | ES_AUTOHSCROLL as u32,
                WS_EX_CLIENTEDGE,
            )?;
            window.control(
                w!("STATIC"),
                "访问地址(&U)：",
                1103,
                ControlBounds {
                    x: 20,
                    y: 122,
                    width: 660,
                    height: 22,
                },
                0,
                0,
            )?;
            window.control(
                w!("EDIT"),
                url,
                URL_ID,
                ControlBounds {
                    x: 20,
                    y: 146,
                    width: 660,
                    height: 28,
                },
                WS_TABSTOP | ES_READONLY as u32 | ES_AUTOHSCROLL as u32,
                WS_EX_CLIENTEDGE,
            )?;
            window.control(
                w!("EDIT"),
                "仅本机可访问。关闭状态窗口将停止服务并退出。",
                STATUS_ID,
                ControlBounds {
                    x: 20,
                    y: 190,
                    width: 660,
                    height: 116,
                },
                WS_TABSTOP
                    | WS_VSCROLL
                    | ES_READONLY as u32
                    | ES_MULTILINE as u32
                    | ES_AUTOVSCROLL as u32,
                WS_EX_CLIENTEDGE,
            )?;
            window.control(
                w!("BUTTON"),
                "重新打开浏览器(&O)",
                OPEN_BROWSER_ID,
                ControlBounds {
                    x: 304,
                    y: 326,
                    width: 190,
                    height: 34,
                },
                WS_TABSTOP | BS_DEFPUSHBUTTON as u32,
                0,
            )?;
            window.control(
                w!("BUTTON"),
                "停止并退出(&X)",
                STOP_ID,
                ControlBounds {
                    x: 510,
                    y: 326,
                    width: 170,
                    height: 34,
                },
                WS_TABSTOP | BS_PUSHBUTTON as u32,
                0,
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

    pub fn notifier(&self) -> Notifier {
        Notifier {
            target: Arc::clone(&self.state.notification),
        }
    }

    pub fn browser_pending(&self) {
        self.set_browser_status(
            false,
            "正在打开默认浏览器。服务仍在运行，可复制上方地址访问。\r\n关闭状态窗口将停止服务并退出。",
        );
    }

    pub fn browser_finished(&self, result: io::Result<()>) {
        let message = match result {
            Ok(()) => {
                "已请求默认浏览器打开访问地址。\r\n仅本机可访问。关闭状态窗口将停止服务并退出。"
                    .to_owned()
            }
            Err(error) => format!(
                "无法打开默认浏览器：{error}\r\n\r\n服务仍在运行。请复制上方访问地址到浏览器，或配置 Windows 默认浏览器后，在状态窗口点击“重新打开浏览器”。"
            ),
        };
        self.set_browser_status(true, &message);
    }

    fn set_browser_status(&self, enabled: bool, message: &str) {
        let message = wide(message);
        // Only the owning GUI thread touches controls. Windows copies the
        // status text synchronously; the read-only edit remains selectable.
        unsafe {
            let hwnd = self.state.hwnd.get();
            if !hwnd.is_null() {
                EnableWindow(GetDlgItem(hwnd, OPEN_BROWSER_ID as i32), i32::from(enabled));
                SetWindowTextW(GetDlgItem(hwnd, STATUS_ID as i32), message.as_ptr());
            }
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

    fn control(
        &self,
        class: PCWSTR,
        text: &str,
        id: u16,
        bounds: ControlBounds,
        style: u32,
        ex_style: u32,
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
                bounds.x,
                bounds.y,
                bounds.width,
                bounds.height,
                self.state.hwnd.get(),
                id as usize as HMENU,
                self._class.instance,
                null(),
            );
            if child.is_null() {
                return Err(win32_error("CreateWindowExW (control)"));
            }
            SendMessageW(child, WM_SETFONT, self.font as usize, 1);
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
            *state.notification.hwnd.lock() = Some(hwnd as usize);
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
                        state.queue_event(event);
                        // BM_CLICK/WM_COMMAND can arrive as synchronous sent
                        // messages while GetMessage is blocked. A posted wakeup
                        // makes it return so next_event observes the Cell too.
                        PostMessageW(hwnd, WM_NULL, 0, 0);
                        return 0;
                    }
                }
                SERVER_STOPPED | BROWSER_FINISHED if wparam == state.notification.id => {
                    state.queue_event(if message == SERVER_STOPPED {
                        Event::ServerStopped
                    } else {
                        Event::BrowserFinished
                    });
                    return 0;
                }
                WM_CLOSE => {
                    DestroyWindow(hwnd);
                    return 0;
                }
                WM_DESTROY => {
                    state.queue_event(Event::Stop);
                    // Do not leave WM_QUIT behind if initialization failed and
                    // its partially created window is being unwound.
                    if state.initialized.get() {
                        PostQuitMessage(0);
                    }
                    return 0;
                }
                WM_NCDESTROY => {
                    *state.notification.hwnd.lock() = None;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queued_worker_completion_cannot_replace_stop() {
        let state = WindowState {
            hwnd: Cell::new(null_mut()),
            event: Cell::new(None),
            initialized: Cell::new(false),
            initialization_error: Cell::new(None),
            notification: Arc::new(NotificationTarget {
                hwnd: Mutex::new(None),
                id: 1,
            }),
        };
        for events in [
            [Event::Stop, Event::BrowserFinished],
            [Event::BrowserFinished, Event::Stop],
            [Event::Stop, Event::ServerStopped],
            [Event::ServerStopped, Event::Stop],
        ] {
            state.event.set(None);
            for event in events {
                state.queue_event(event);
            }
            assert_eq!(state.event.take(), Some(Event::Stop));
        }
    }
}

#[cfg(test)]
mod browser_behavior_tests {
    use super::*;
    use crate::BrowserActivation;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::IsWindowEnabled;

    // The process-wide native class is deliberately registered only once at a
    // time. Each case owns a fresh GUI thread, including its WM_QUIT queue.
    static GUI_TEST_LOCK: Mutex<()> = Mutex::new(());

    fn control_text(window: &StatusWindow, id: u16) -> String {
        unsafe {
            let control = GetDlgItem(window.state.hwnd.get(), id as i32);
            let mut text = vec![0; GetWindowTextLengthW(control) as usize + 1];
            let length = GetWindowTextW(control, text.as_mut_ptr(), text.len() as i32);
            String::from_utf16(&text[..length as usize]).unwrap()
        }
    }

    #[test]
    fn browser_failure_arrives_on_gui_thread_and_retry_updates_inline_status() {
        let _guard = GUI_TEST_LOCK.lock();
        std::thread::spawn(|| {
            let url = "http://127.0.0.1:12345/";
            let window = StatusWindow::new(&std::env::temp_dir(), url).unwrap();
            let mut browser = BrowserActivation::new();
            browser.start_with(url, &window, move |_| {
                Err(io::Error::other("browser failure from worker"))
            });
            assert_eq!(window.next_event().unwrap(), Event::BrowserFinished);
            browser.finish(&window);
            assert!(control_text(&window, STATUS_ID).contains("browser failure from worker"));
            assert_eq!(control_text(&window, URL_ID), url);
            assert_ne!(
                unsafe {
                    IsWindowEnabled(GetDlgItem(window.state.hwnd.get(), OPEN_BROWSER_ID as i32))
                },
                0
            );

            browser.start_with(url, &window, |_| Ok(()));
            assert_eq!(window.next_event().unwrap(), Event::BrowserFinished);
            browser.finish(&window);
            let status = control_text(&window, STATUS_ID);
            assert!(!status.contains("browser failure from worker"));
        })
        .join()
        .unwrap();
    }

    #[test]
    fn stop_button_and_close_remain_responsive_during_blocked_browser_activation() {
        let _guard = GUI_TEST_LOCK.lock();
        for close in [false, true] {
            std::thread::spawn(move || {
                let url = "http://127.0.0.1:12345/";
                let window = StatusWindow::new(&std::env::temp_dir(), url).unwrap();
                let mut browser = BrowserActivation::new();
                let (entered_tx, entered_rx) = mpsc::channel();
                let (release_tx, release_rx) = mpsc::channel();
                let started = Instant::now();
                browser.start_with(url, &window, move |_| {
                    entered_tx.send(()).unwrap();
                    let _ = release_rx.recv_timeout(Duration::from_secs(5));
                    Err(io::Error::other("blocked browser eventually returned"))
                });
                assert!(started.elapsed() < Duration::from_secs(1));
                entered_rx.recv_timeout(Duration::from_secs(1)).unwrap();
                assert_eq!(
                    unsafe {
                        IsWindowEnabled(GetDlgItem(window.state.hwnd.get(), OPEN_BROWSER_ID as i32))
                    },
                    0
                );
                let (unexpected_tx, unexpected_rx) = mpsc::channel();
                browser.start_with(url, &window, move |_| {
                    let _ = unexpected_tx.send(());
                    Ok(())
                });
                assert!(matches!(
                    unexpected_rx.try_recv(),
                    Err(mpsc::TryRecvError::Disconnected)
                ));
                unsafe {
                    let hwnd = window.state.hwnd.get();
                    let posted = if close {
                        PostMessageW(hwnd, WM_CLOSE, 0, 0)
                    } else {
                        PostMessageW(hwnd, WM_COMMAND, STOP_ID as usize, 0)
                    };
                    assert_ne!(posted, 0);
                }
                let started = Instant::now();
                assert_eq!(window.next_event().unwrap(), Event::Stop);
                assert!(matches!(
                    browser.result_rx.try_recv(),
                    Err(mpsc::TryRecvError::Empty)
                ));
                drop(browser);
                drop(window);
                assert!(started.elapsed() < Duration::from_secs(1));
                let _ = release_tx.send(());
            })
            .join()
            .unwrap();
        }
    }
}
