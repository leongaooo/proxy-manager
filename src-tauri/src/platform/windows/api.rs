use std::mem::size_of;

use windows::Win32::Foundation::{
    ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_SUCCESS, LPARAM, WPARAM,
};
use windows::Win32::Networking::WinInet::{
    INTERNET_OPTION_REFRESH, INTERNET_OPTION_SETTINGS_CHANGED, InternetSetOptionW,
};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_DWORD, REG_EXPAND_SZ, REG_SAM_FLAGS, REG_SZ,
    REG_VALUE_TYPE, RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    HWND_BROADCAST, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_SETTINGCHANGE,
};
use windows::core::PCWSTR;

use super::WindowsApi;
use crate::contracts::{ProxyError, ProxyErrorCode};

#[derive(Clone, Copy)]
pub(super) enum RegistryArea {
    Environment,
    InternetSettings,
}

pub(super) struct RealWindowsApi;

struct RegistryKey(HKEY);

impl Drop for RegistryKey {
    fn drop(&mut self) {
        // 关闭句柄失败没有可恢复动作，原始读写结果已经确定。
        let _ = unsafe { RegCloseKey(self.0) };
    }
}

impl WindowsApi for RealWindowsApi {
    fn read_string(&self, area: RegistryArea, name: &str) -> Result<Option<String>, ProxyError> {
        read_registry_string(area, name)
    }

    fn write_string(&self, area: RegistryArea, name: &str, value: &str) -> Result<(), ProxyError> {
        write_registry_string(area, name, value)
    }

    fn delete_value(&self, area: RegistryArea, name: &str) -> Result<(), ProxyError> {
        delete_registry_value(area, name)
    }

    fn read_dword(&self, area: RegistryArea, name: &str) -> Result<Option<u32>, ProxyError> {
        read_registry_dword(area, name)
    }

    fn write_dword(&self, area: RegistryArea, name: &str, value: u32) -> Result<(), ProxyError> {
        write_registry_dword(area, name, value)
    }

    fn broadcast_environment_change(&self) -> Result<(), ProxyError> {
        broadcast_environment_change()
    }

    fn refresh_internet_settings(&self) -> Result<(), ProxyError> {
        refresh_internet_settings()
    }
}

fn read_registry_string(area: RegistryArea, name: &str) -> Result<Option<String>, ProxyError> {
    let key = open_registry_key(area, KEY_READ, "读取")?;
    let name_wide = wide_null(name);
    let mut kind = REG_VALUE_TYPE::default();
    let mut byte_count = 0_u32;
    let status = unsafe {
        RegQueryValueExW(
            key.0,
            PCWSTR(name_wide.as_ptr()),
            None,
            Some(&mut kind),
            None,
            Some(&mut byte_count),
        )
    };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    ensure_registry_success(status, "读取", registry_label(area), name)?;
    if kind != REG_SZ && kind != REG_EXPAND_SZ {
        return Err(unsupported_value_type(area, name));
    }
    if byte_count == 0 {
        return Ok(Some(String::new()));
    }

    let mut buffer = vec![0_u16; byte_count.div_ceil(2) as usize + 1];
    let status = unsafe {
        RegQueryValueExW(
            key.0,
            PCWSTR(name_wide.as_ptr()),
            None,
            Some(&mut kind),
            Some(buffer.as_mut_ptr().cast::<u8>()),
            Some(&mut byte_count),
        )
    };
    ensure_registry_success(status, "读取", registry_label(area), name)?;
    let used_units = byte_count as usize / 2;
    buffer.truncate(used_units);
    while buffer.last() == Some(&0) {
        buffer.pop();
    }
    String::from_utf16(&buffer).map(Some).map_err(|_| {
        ProxyError::read(format!(
            "{}中的 {name} 含无效 UTF-16 数据；请在 Windows 环境或代理设置中修正后重新读取",
            registry_label(area)
        ))
    })
}

fn write_registry_string(area: RegistryArea, name: &str, value: &str) -> Result<(), ProxyError> {
    let key = open_registry_key(area, KEY_WRITE, "写入")?;
    let name_wide = wide_null(name);
    let existing_type = registry_value_type(&key, &name_wide, area, name)?;
    let kind = match existing_type {
        None | Some(REG_SZ) => REG_SZ,
        Some(REG_EXPAND_SZ) => REG_EXPAND_SZ,
        Some(_) => return Err(unsupported_value_type(area, name)),
    };
    let value_wide = wide_null(value);
    let bytes = unsafe {
        std::slice::from_raw_parts(value_wide.as_ptr().cast::<u8>(), value_wide.len() * 2)
    };
    let status =
        unsafe { RegSetValueExW(key.0, PCWSTR(name_wide.as_ptr()), None, kind, Some(bytes)) };
    ensure_registry_success(status, "写入", registry_label(area), name)
}

fn delete_registry_value(area: RegistryArea, name: &str) -> Result<(), ProxyError> {
    let key = open_registry_key(area, KEY_WRITE, "删除")?;
    let name_wide = wide_null(name);
    let status = unsafe { RegDeleteValueW(key.0, PCWSTR(name_wide.as_ptr())) };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(());
    }
    ensure_registry_success(status, "删除", registry_label(area), name)
}

fn read_registry_dword(area: RegistryArea, name: &str) -> Result<Option<u32>, ProxyError> {
    let key = open_registry_key(area, KEY_READ, "读取")?;
    let name_wide = wide_null(name);
    let mut kind = REG_VALUE_TYPE::default();
    let mut value = 0_u32;
    let mut byte_count = size_of::<u32>() as u32;
    let status = unsafe {
        RegQueryValueExW(
            key.0,
            PCWSTR(name_wide.as_ptr()),
            None,
            Some(&mut kind),
            Some((&mut value as *mut u32).cast::<u8>()),
            Some(&mut byte_count),
        )
    };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    ensure_registry_success(status, "读取", registry_label(area), name)?;
    if kind != REG_DWORD || byte_count != size_of::<u32>() as u32 {
        return Err(unsupported_value_type(area, name));
    }
    Ok(Some(value))
}

fn write_registry_dword(area: RegistryArea, name: &str, value: u32) -> Result<(), ProxyError> {
    let key = open_registry_key(area, KEY_WRITE, "写入")?;
    let name_wide = wide_null(name);
    let status = unsafe {
        RegSetValueExW(
            key.0,
            PCWSTR(name_wide.as_ptr()),
            None,
            REG_DWORD,
            Some(&value.to_le_bytes()),
        )
    };
    ensure_registry_success(status, "写入", registry_label(area), name)
}

fn registry_value_type(
    key: &RegistryKey,
    name_wide: &[u16],
    area: RegistryArea,
    name: &str,
) -> Result<Option<REG_VALUE_TYPE>, ProxyError> {
    let mut kind = REG_VALUE_TYPE::default();
    let mut byte_count = 0_u32;
    let status = unsafe {
        RegQueryValueExW(
            key.0,
            PCWSTR(name_wide.as_ptr()),
            None,
            Some(&mut kind),
            None,
            Some(&mut byte_count),
        )
    };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    ensure_registry_success(status, "读取", registry_label(area), name)?;
    Ok(Some(kind))
}

fn open_registry_key(
    area: RegistryArea,
    access: REG_SAM_FLAGS,
    action: &str,
) -> Result<RegistryKey, ProxyError> {
    let path = match area {
        RegistryArea::Environment => "Environment",
        RegistryArea::InternetSettings => {
            "Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings"
        }
    };
    let path_wide = wide_null(path);
    let mut key = HKEY::default();
    let status = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(path_wide.as_ptr()),
            None,
            access,
            &mut key,
        )
    };
    ensure_registry_success(status, action, registry_label(area), "注册表项")?;
    Ok(RegistryKey(key))
}

fn ensure_registry_success(
    status: windows::Win32::Foundation::WIN32_ERROR,
    action: &str,
    area: &str,
    name: &str,
) -> Result<(), ProxyError> {
    if status == ERROR_SUCCESS || status == ERROR_MORE_DATA {
        return Ok(());
    }
    let permission_denied = status == windows::Win32::Foundation::ERROR_ACCESS_DENIED;
    Err(ProxyError::new(
        if permission_denied {
            ProxyErrorCode::PermissionDenied
        } else if action == "读取" {
            ProxyErrorCode::ReadFailed
        } else {
            ProxyErrorCode::PersistFailed
        },
        if permission_denied {
            format!(
                "没有权限{action}{area}中的 {name}；请使用当前 Windows 用户检查该项权限后重试，本应用不会写入机器级配置"
            )
        } else {
            format!(
                "{action}{area}中的 {name} 失败；请重新读取实际配置并检查该项是否可访问，本应用未改用其他权限范围"
            )
        },
    ))
}

fn unsupported_value_type(area: RegistryArea, name: &str) -> ProxyError {
    ProxyError::unsupported(format!(
        "{}中的 {name} 不是本应用支持的文本或数值类型；请先备份并在 Windows 对应设置中修正，再重新读取",
        registry_label(area)
    ))
}

fn registry_label(area: RegistryArea) -> &'static str {
    match area {
        RegistryArea::Environment => "Windows 当前用户环境",
        RegistryArea::InternetSettings => "Windows 当前用户 WinINet 设置",
    }
}

fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn broadcast_environment_change() -> Result<(), ProxyError> {
    let environment = wide_null("Environment");
    let mut result = 0_usize;
    let sent = unsafe {
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            WPARAM(0),
            LPARAM(environment.as_ptr() as isize),
            SMTO_ABORTIFHUNG,
            5_000,
            Some(&mut result),
        )
    };
    if sent.0 == 0 {
        return Err(ProxyError::new(
            ProxyErrorCode::NotifyFailed,
            "环境变更广播未完成",
        ));
    }
    Ok(())
}

fn refresh_internet_settings() -> Result<(), ProxyError> {
    for option in [INTERNET_OPTION_SETTINGS_CHANGED, INTERNET_OPTION_REFRESH] {
        let result = unsafe { InternetSetOptionW(None, option, None, 0) };
        if result.is_err() {
            return Err(ProxyError::new(
                ProxyErrorCode::NotifyFailed,
                "WinINet 设置刷新未完成",
            ));
        }
    }
    Ok(())
}
