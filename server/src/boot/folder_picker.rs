use std::path::PathBuf;

#[cfg(windows)]
pub fn pick_folder() -> Result<Option<PathBuf>, String> {
    use std::os::windows::ffi::OsStringExt;
    use windows::core::PCWSTR;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Shell::{
        IFileOpenDialog, SIGDN_FILESYSPATH, FOS_FORCEFILESYSTEM, FOS_PATHMUSTEXIST,
        FOS_PICKFOLDERS, FileOpenDialog,
    };

    struct ComApartment;
    impl Drop for ComApartment {
        fn drop(&mut self) {
            unsafe { CoUninitialize() };
        }
    }

    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED)
            .ok()
            .map_err(|error| format!("无法初始化 Windows 文件夹选择器：{error}"))?;
    }
    let _apartment = ComApartment;

    let dialog: IFileOpenDialog = unsafe {
        CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)
            .map_err(|error| format!("无法创建 Windows 文件夹选择器：{error}"))?
    };
    let mut options = unsafe {
        dialog
            .GetOptions()
            .map_err(|error| format!("无法读取文件夹选择器选项：{error}"))?
    };
    options |= FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST;
    unsafe {
        dialog
            .SetOptions(options)
            .map_err(|error| format!("无法设置文件夹选择器选项：{error}"))?;
        let title: Vec<u16> = "选择要授权给 115 模块的本地文件夹\0".encode_utf16().collect();
        dialog
            .SetTitle(PCWSTR::from_raw(title.as_ptr()))
            .map_err(|error| format!("无法设置文件夹选择器标题：{error}"))?;
    }

    if let Err(error) = unsafe { dialog.Show(None) } {
        // HRESULT_FROM_WIN32(ERROR_CANCELLED): cancel is not an error.
        if error.code().0 as u32 == 0x8007_04C7 {
            return Ok(None);
        }
        return Err(format!("Windows 文件夹选择器失败：{error}"));
    }

    let item = unsafe {
        dialog
            .GetResult()
            .map_err(|error| format!("无法读取所选文件夹：{error}"))?
    };
    let display_name = unsafe {
        item.GetDisplayName(SIGDN_FILESYSPATH)
            .map_err(|error| format!("所选项目不是本地文件夹：{error}"))?
    };
    let path = unsafe { PathBuf::from(std::ffi::OsString::from_wide(display_name.as_wide())) };
    unsafe { windows::Win32::System::Com::CoTaskMemFree(Some(display_name.0.cast())) };
    Ok(Some(path))
}

#[cfg(not(windows))]
pub fn pick_folder() -> Result<Option<PathBuf>, String> {
    Err("本地原生文件夹选择器仅支持 Windows 桌面版".into())
}
