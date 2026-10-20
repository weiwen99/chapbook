//! Windows 默认应用打开接口: 文件路径与浏览器 URL 共用，无 shell 命令拼接。

use std::ffi::OsStr;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows_sys::Win32::System::Com::{
    COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
};
use windows_sys::Win32::UI::Shell::{
    SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW, ShellExecuteExW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

/// 通过系统关联打开路径或 URL。调用方负责授权与目标校验；不执行 shell。
pub fn open(target: &OsStr) -> io::Result<()> {
    // Shell API 不支持通常的 canonicalize verbatim 前缀; 仅在无损时简化。
    // 普通路径和 http URL 均原样保留。
    let file: Vec<u16> = dunce::simplified(Path::new(target))
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_FLAG_NO_UI | SEE_MASK_NOASYNC,
        lpFile: file.as_ptr(),
        nShow: SW_SHOWNORMAL,
        ..Default::default()
    };
    // SAFETY: COM 初始化/反初始化在同一线程配对; file 在同步调用期间存活，
    // 是 NUL 结尾的 UTF-16。不请求进程 handle，默认应用生命周期由系统管理。
    unsafe {
        let initialized = CoInitializeEx(
            std::ptr::null(),
            (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32,
        );
        if initialized < 0 {
            return Err(io::Error::other(format!(
                "CoInitializeEx failed: {initialized:#x}"
            )));
        }
        let result = if ShellExecuteExW(&mut info) == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        };
        CoUninitialize();
        result
    }
}
