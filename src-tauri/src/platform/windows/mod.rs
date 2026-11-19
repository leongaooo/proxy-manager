mod api;

use crate::contracts::{
    Platform, ProxyError, ProxyErrorCode, ProxySnapshot, SectionResult, SystemProxyInput,
    SystemSnapshot, TerminalProxyInput, TerminalSnapshot, TerminalValues,
};
use crate::platform::PlatformAdapter;
use crate::rules::{
    TERMINAL_VARIABLES, format_windows_proxy_server, parse_windows_proxy_server,
    sanitize_terminal_values,
};
use api::{RealWindowsApi, RegistryArea};

/// Windows 仅管理当前用户环境及 WinINet；不会写 HKLM、WinHTTP 或其他连接配置。
pub struct WindowsAdapter;

trait WindowsApi: Send + Sync {
    fn read_string(&self, area: RegistryArea, name: &str) -> Result<Option<String>, ProxyError>;
    fn write_string(&self, area: RegistryArea, name: &str, value: &str) -> Result<(), ProxyError>;
    fn delete_value(&self, area: RegistryArea, name: &str) -> Result<(), ProxyError>;
    fn read_dword(&self, area: RegistryArea, name: &str) -> Result<Option<u32>, ProxyError>;
    fn write_dword(&self, area: RegistryArea, name: &str, value: u32) -> Result<(), ProxyError>;
    fn broadcast_environment_change(&self) -> Result<(), ProxyError>;
    fn refresh_internet_settings(&self) -> Result<(), ProxyError>;
}

impl PlatformAdapter for WindowsAdapter {
    fn read_snapshot(&self, _target: Option<&str>) -> Result<ProxySnapshot, ProxyError> {
        read_snapshot(&RealWindowsApi)
    }

    fn apply_terminal(&self, input: &TerminalProxyInput) -> Result<SectionResult, ProxyError> {
        apply_terminal(&RealWindowsApi, input)
    }

    fn apply_system(&self, input: &SystemProxyInput) -> Result<SectionResult, ProxyError> {
        apply_system(&RealWindowsApi, input)
    }
}

struct WindowsSystemState {
    enabled: bool,
    proxy_server: String,
    proxy_override: String,
    snapshot: SystemSnapshot,
}

fn read_snapshot(api: &dyn WindowsApi) -> Result<ProxySnapshot, ProxyError> {
    let persisted = read_terminal_values(api)?;
    let system = read_system_state(api)?;
    let (process, mut warnings) = process_environment();
    for (source, values) in [("应用进程", &process), ("Windows 用户环境", &persisted)] {
        if has_authenticated_proxy(values) {
            warnings.push(format!(
                "{source}中存在认证代理，已脱敏展示；本期不支持认证代理，请移除账号密码后再保存对应变量"
            ));
        }
    }
    Ok(ProxySnapshot {
        platform: Platform::Windows,
        terminal: TerminalSnapshot {
            process: sanitize_terminal_values(&process),
            persisted: sanitize_terminal_values(&persisted),
            persistence_label: "Windows 当前用户环境变量（HKCU\\Environment）".to_owned(),
            launchd: None,
            warnings,
        },
        system: system.snapshot,
        warnings: Vec::new(),
    })
}

fn process_environment() -> (TerminalValues, Vec<String>) {
    let mut values = TerminalValues::default();
    let mut warnings = Vec::new();
    for name in TERMINAL_VARIABLES {
        match std::env::var(name) {
            Ok(value) => set_terminal_value(&mut values, name, Some(value)),
            Err(std::env::VarError::NotPresent) => {}
            Err(std::env::VarError::NotUnicode(_)) => warnings.push(format!(
                "应用进程中的 {name} 不是有效 Unicode，快照未展示该值；请在新终端中检查并修正该环境变量"
            )),
        }
    }
    (values, warnings)
}

fn has_authenticated_proxy(values: &TerminalValues) -> bool {
    [&values.http_proxy, &values.https_proxy, &values.all_proxy]
        .into_iter()
        .flatten()
        .any(|value| {
            url::Url::parse(value)
                .map(|url| !url.username().is_empty() || url.password().is_some())
                .unwrap_or_else(|_| value.contains('@'))
        })
}

fn read_terminal_values(api: &dyn WindowsApi) -> Result<TerminalValues, ProxyError> {
    let mut values = TerminalValues::default();
    for name in TERMINAL_VARIABLES {
        let value = api.read_string(RegistryArea::Environment, name)?;
        set_terminal_value(&mut values, name, value);
    }
    Ok(values)
}

fn apply_terminal(
    api: &dyn WindowsApi,
    input: &TerminalProxyInput,
) -> Result<SectionResult, ProxyError> {
    let before = read_terminal_values(api)?;
    let desired = merge_terminal_values(&before, input);
    if desired == before {
        return Ok(SectionResult::unchanged(
            "Windows 当前用户终端代理已符合请求，未重复写入或广播环境变更",
        ));
    }

    for name in TERMINAL_VARIABLES {
        let previous = terminal_value(&before, name);
        let next = terminal_value(&desired, name);
        if previous == next {
            continue;
        }
        match next {
            Some(value) => api.write_string(RegistryArea::Environment, name, value)?,
            None => api.delete_value(RegistryArea::Environment, name)?,
        }
    }

    // 注册表已更新但通知失败不能伪装成写入失败；新终端通常仍可获得持久值。
    api.broadcast_environment_change().map_err(|_| {
        ProxyError::new(
            ProxyErrorCode::NotifyFailed,
            "终端代理已写入当前用户环境，但系统环境变更通知未完成；请重新读取确认持久值，并关闭后重新打开终端。已运行的终端不会被本应用热更新",
        )
    })?;

    let after = read_terminal_values(api)?;
    if after != desired {
        return Err(ProxyError::verify(
            "Windows 当前用户环境保存后回读与请求不一致；请检查是否有其他程序同时修改环境变量，重新读取实际值后再决定是否需要重试",
        ));
    }
    Ok(SectionResult::applied(
        "终端代理已写入当前用户环境并回读一致，系统已收到环境变更通知；新终端生效，已运行的终端需重开或手动刷新",
    ))
}

fn merge_terminal_values(current: &TerminalValues, input: &TerminalProxyInput) -> TerminalValues {
    TerminalValues {
        http_proxy: merge_terminal_value(&current.http_proxy, &input.http_proxy),
        https_proxy: merge_terminal_value(&current.https_proxy, &input.https_proxy),
        all_proxy: merge_terminal_value(&current.all_proxy, &input.all_proxy),
        no_proxy: merge_terminal_value(&current.no_proxy, &input.no_proxy),
    }
}

fn merge_terminal_value(current: &Option<String>, input: &Option<String>) -> Option<String> {
    match input {
        None => current.clone(),
        Some(value) if value.is_empty() => None,
        Some(value) => Some(value.clone()),
    }
}

fn terminal_value<'a>(values: &'a TerminalValues, name: &str) -> &'a Option<String> {
    match name {
        "HTTP_PROXY" => &values.http_proxy,
        "HTTPS_PROXY" => &values.https_proxy,
        "ALL_PROXY" => &values.all_proxy,
        "NO_PROXY" => &values.no_proxy,
        _ => unreachable!("only known terminal variables are passed"),
    }
}

fn set_terminal_value(values: &mut TerminalValues, name: &str, value: Option<String>) {
    match name {
        "HTTP_PROXY" => values.http_proxy = value,
        "HTTPS_PROXY" => values.https_proxy = value,
        "ALL_PROXY" => values.all_proxy = value,
        "NO_PROXY" => values.no_proxy = value,
        _ => unreachable!("only known terminal variables are passed"),
    }
}

fn read_system_state(api: &dyn WindowsApi) -> Result<WindowsSystemState, ProxyError> {
    let enabled = api
        .read_dword(RegistryArea::InternetSettings, "ProxyEnable")?
        .unwrap_or(0)
        != 0;
    let proxy_server = api
        .read_string(RegistryArea::InternetSettings, "ProxyServer")?
        .unwrap_or_default();
    let proxy_override = api
        .read_string(RegistryArea::InternetSettings, "ProxyOverride")?
        .unwrap_or_default();
    let parsed = parse_windows_proxy_server(&proxy_server)?;
    let auto_config_url = api
        .read_string(RegistryArea::InternetSettings, "AutoConfigURL")?
        .filter(|value| !value.trim().is_empty());
    let auto_detect = api
        .read_dword(RegistryArea::InternetSettings, "AutoDetect")?
        .unwrap_or(0)
        != 0;
    let auto_config = match (auto_config_url.is_some(), auto_detect) {
        (true, true) => Some("自动代理配置（PAC）和自动检测（WPAD）".to_owned()),
        (true, false) => Some("自动代理配置（PAC）".to_owned()),
        (false, true) => Some("自动检测（WPAD）".to_owned()),
        (false, false) => None,
    };
    let mut warnings = Vec::new();
    if let Some(config) = &auto_config {
        warnings.push(format!(
            "Windows 当前用户已启用{config}，与手动代理可能冲突；启用保存前请在系统设置中关闭该自动配置，本应用不会擅自清除"
        ));
    }
    let bypass = proxy_override
        .split(';')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect();
    Ok(WindowsSystemState {
        enabled,
        proxy_server,
        proxy_override,
        snapshot: SystemSnapshot {
            enabled,
            http: parsed.http,
            https: parsed.https,
            socks: parsed.socks,
            bypass,
            available_targets: Vec::new(),
            selected_target: None,
            auto_config,
            warnings,
        },
    })
}

fn apply_system(
    api: &dyn WindowsApi,
    input: &SystemProxyInput,
) -> Result<SectionResult, ProxyError> {
    let before = read_system_state(api)?;
    if input.enabled && before.snapshot.auto_config.is_some() {
        let config = before
            .snapshot
            .auto_config
            .as_deref()
            .unwrap_or("自动代理配置");
        return Err(ProxyError::unsupported(format!(
            "Windows 当前用户已启用{config}，与手动系统代理冲突；请先在 Windows 设置 → 网络和 Internet → 代理中关闭该自动配置，再重新读取并保存。本应用未修改系统代理"
        )));
    }

    let desired_server = input.enabled.then(|| format_windows_proxy_server(input));
    let desired_override = input.enabled.then(|| input.bypass.join(";"));
    let changed = before.enabled != input.enabled
        || desired_server
            .as_ref()
            .is_some_and(|value| value != &before.proxy_server)
        || desired_override
            .as_ref()
            .is_some_and(|value| value != &before.proxy_override);
    if !changed {
        return Ok(SectionResult::unchanged(if input.enabled {
            "Windows 当前用户 WinINet 手动代理已符合请求，未重复写入"
        } else {
            "Windows 当前用户 WinINet 手动代理已关闭，服务器与绕过配置保持原样"
        }));
    }

    // 先准备服务器和绕过值，最后再开启开关，避免启用一个不完整的新配置。
    if let Some(server) = &desired_server
        && server != &before.proxy_server
    {
        api.write_string(RegistryArea::InternetSettings, "ProxyServer", server)?;
    }
    if let Some(overrides) = &desired_override
        && overrides != &before.proxy_override
    {
        api.write_string(RegistryArea::InternetSettings, "ProxyOverride", overrides)?;
    }
    if before.enabled != input.enabled {
        api.write_dword(
            RegistryArea::InternetSettings,
            "ProxyEnable",
            u32::from(input.enabled),
        )?;
    }

    api.refresh_internet_settings().map_err(|_| {
        ProxyError::new(
            ProxyErrorCode::NotifyFailed,
            "WinINet 代理值已写入，但系统代理刷新通知未完成；请重新读取实际配置，必要时重新打开使用系统代理的应用。本应用未修改 WinHTTP 或 PAC",
        )
    })?;

    let after = read_system_state(api)?;
    verify_system(input, &before, &after)?;
    Ok(SectionResult::applied(if input.enabled {
        "Windows 当前用户 WinINet 手动代理已更新、刷新并回读一致；保存成功不代表代理服务器可连接，WinHTTP 未被修改"
    } else {
        "Windows 当前用户 WinINet 手动代理已关闭、刷新并回读一致；服务器与绕过配置保持原样，WinHTTP 未被修改"
    }))
}

fn verify_system(
    input: &SystemProxyInput,
    before: &WindowsSystemState,
    after: &WindowsSystemState,
) -> Result<(), ProxyError> {
    let matches = if input.enabled {
        let expected_server = format_windows_proxy_server(input);
        let expected_override = input.bypass.join(";");
        after.enabled
            && after.proxy_server == expected_server
            && after.proxy_override == expected_override
    } else {
        !after.enabled
            && after.proxy_server == before.proxy_server
            && after.proxy_override == before.proxy_override
    };
    if matches {
        Ok(())
    } else {
        Err(ProxyError::verify(
            "WinINet 系统代理保存后回读与请求不一致；请检查是否有其他程序同时修改 Windows 代理，重新读取实际值后再决定是否需要重试",
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use super::*;
    use crate::contracts::ProxyEndpoint;

    #[derive(Default)]
    struct FakeWindowsApi {
        strings: Mutex<HashMap<(u8, String), String>>,
        dwords: Mutex<HashMap<(u8, String), u32>>,
        writes: Mutex<Vec<String>>,
        fail_notify: bool,
    }

    impl FakeWindowsApi {
        fn key(area: RegistryArea, name: &str) -> (u8, String) {
            (
                match area {
                    RegistryArea::Environment => 0,
                    RegistryArea::InternetSettings => 1,
                },
                name.to_owned(),
            )
        }

        fn set_string(&self, area: RegistryArea, name: &str, value: &str) {
            self.strings
                .lock()
                .unwrap()
                .insert(Self::key(area, name), value.to_owned());
        }

        fn set_dword(&self, area: RegistryArea, name: &str, value: u32) {
            self.dwords
                .lock()
                .unwrap()
                .insert(Self::key(area, name), value);
        }
    }

    impl WindowsApi for FakeWindowsApi {
        fn read_string(
            &self,
            area: RegistryArea,
            name: &str,
        ) -> Result<Option<String>, ProxyError> {
            Ok(self
                .strings
                .lock()
                .unwrap()
                .get(&Self::key(area, name))
                .cloned())
        }

        fn write_string(
            &self,
            area: RegistryArea,
            name: &str,
            value: &str,
        ) -> Result<(), ProxyError> {
            self.set_string(area, name, value);
            self.writes.lock().unwrap().push(format!("write:{name}"));
            Ok(())
        }

        fn delete_value(&self, area: RegistryArea, name: &str) -> Result<(), ProxyError> {
            self.strings.lock().unwrap().remove(&Self::key(area, name));
            self.writes.lock().unwrap().push(format!("delete:{name}"));
            Ok(())
        }

        fn read_dword(&self, area: RegistryArea, name: &str) -> Result<Option<u32>, ProxyError> {
            Ok(self
                .dwords
                .lock()
                .unwrap()
                .get(&Self::key(area, name))
                .copied())
        }

        fn write_dword(
            &self,
            area: RegistryArea,
            name: &str,
            value: u32,
        ) -> Result<(), ProxyError> {
            self.set_dword(area, name, value);
            self.writes.lock().unwrap().push(format!("dword:{name}"));
            Ok(())
        }

        fn broadcast_environment_change(&self) -> Result<(), ProxyError> {
            self.writes.lock().unwrap().push("notify".to_owned());
            if self.fail_notify {
                Err(ProxyError::new(ProxyErrorCode::NotifyFailed, "fake"))
            } else {
                Ok(())
            }
        }

        fn refresh_internet_settings(&self) -> Result<(), ProxyError> {
            self.writes.lock().unwrap().push("refresh".to_owned());
            Ok(())
        }
    }

    fn endpoint(host: &str, port: u16) -> ProxyEndpoint {
        ProxyEndpoint {
            host: host.to_owned(),
            port,
        }
    }

    #[test]
    fn apply_terminal_should_merge_delete_verify_and_notify() {
        let api = FakeWindowsApi::default();
        api.set_string(
            RegistryArea::Environment,
            "HTTP_PROXY",
            "http://old.local:8080",
        );
        api.set_string(
            RegistryArea::Environment,
            "HTTPS_PROXY",
            "http://keep.local:8080",
        );
        let result = apply_terminal(
            &api,
            &TerminalProxyInput {
                http_proxy: Some("http://127.0.0.1:7890".to_owned()),
                https_proxy: None,
                all_proxy: None,
                no_proxy: Some("localhost,127.0.0.1".to_owned()),
            },
        )
        .expect("terminal apply should succeed");
        assert!(result.message.contains("回读一致"));
        let saved = read_terminal_values(&api).unwrap();
        assert_eq!(saved.http_proxy.as_deref(), Some("http://127.0.0.1:7890"));
        assert_eq!(saved.https_proxy.as_deref(), Some("http://keep.local:8080"));
        assert_eq!(saved.no_proxy.as_deref(), Some("localhost,127.0.0.1"));
        assert_eq!(
            api.writes.lock().unwrap().last().map(String::as_str),
            Some("notify")
        );
    }

    #[test]
    fn apply_terminal_should_report_notification_failure_after_persisting() {
        let api = FakeWindowsApi {
            fail_notify: true,
            ..FakeWindowsApi::default()
        };
        let error = apply_terminal(
            &api,
            &TerminalProxyInput {
                http_proxy: Some("http://127.0.0.1:7890".to_owned()),
                ..TerminalProxyInput::default()
            },
        )
        .unwrap_err();
        assert_eq!(error.code, ProxyErrorCode::NotifyFailed);
        assert!(error.message.contains("已写入"));
        assert_eq!(
            read_terminal_values(&api).unwrap().http_proxy.as_deref(),
            Some("http://127.0.0.1:7890")
        );
    }

    #[test]
    fn apply_system_should_write_server_before_enable_and_verify() {
        let api = FakeWindowsApi::default();
        let result = apply_system(
            &api,
            &SystemProxyInput {
                enabled: true,
                http: Some(endpoint("127.0.0.1", 7890)),
                https: Some(endpoint("127.0.0.1", 7890)),
                socks: None,
                bypass: vec!["localhost".to_owned(), "127.0.0.1".to_owned()],
                target: None,
            },
        )
        .expect("system apply should succeed");
        assert!(result.message.contains("回读一致"));
        assert_eq!(
            api.read_string(RegistryArea::InternetSettings, "ProxyServer")
                .unwrap()
                .as_deref(),
            Some("127.0.0.1:7890")
        );
        assert_eq!(
            api.read_dword(RegistryArea::InternetSettings, "ProxyEnable")
                .unwrap(),
            Some(1)
        );
        let writes = api.writes.lock().unwrap();
        let server_index = writes
            .iter()
            .position(|entry| entry == "write:ProxyServer")
            .unwrap();
        let enable_index = writes
            .iter()
            .position(|entry| entry == "dword:ProxyEnable")
            .unwrap();
        assert!(server_index < enable_index);
        assert_eq!(writes.last().map(String::as_str), Some("refresh"));
    }

    #[test]
    fn apply_system_should_disable_without_deleting_server_or_bypass() {
        let api = FakeWindowsApi::default();
        api.set_dword(RegistryArea::InternetSettings, "ProxyEnable", 1);
        api.set_string(
            RegistryArea::InternetSettings,
            "ProxyServer",
            "http=127.0.0.1:7890;https=127.0.0.1:7890",
        );
        api.set_string(RegistryArea::InternetSettings, "ProxyOverride", "localhost");
        apply_system(
            &api,
            &SystemProxyInput {
                enabled: false,
                http: None,
                https: None,
                socks: None,
                bypass: Vec::new(),
                target: None,
            },
        )
        .expect("disable should succeed");
        assert_eq!(
            api.read_dword(RegistryArea::InternetSettings, "ProxyEnable")
                .unwrap(),
            Some(0)
        );
        assert!(
            api.read_string(RegistryArea::InternetSettings, "ProxyServer")
                .unwrap()
                .is_some()
        );
        assert_eq!(
            api.read_string(RegistryArea::InternetSettings, "ProxyOverride")
                .unwrap()
                .as_deref(),
            Some("localhost")
        );
        assert!(
            !api.writes
                .lock()
                .unwrap()
                .iter()
                .any(|entry| entry == "write:ProxyServer")
        );
    }

    #[test]
    fn apply_system_should_block_auto_configuration_before_writes() {
        let api = FakeWindowsApi::default();
        api.set_string(
            RegistryArea::InternetSettings,
            "AutoConfigURL",
            "http://config.local/proxy.pac",
        );
        let error = apply_system(
            &api,
            &SystemProxyInput {
                enabled: true,
                http: Some(endpoint("127.0.0.1", 7890)),
                https: None,
                socks: None,
                bypass: Vec::new(),
                target: None,
            },
        )
        .unwrap_err();
        assert_eq!(error.code, ProxyErrorCode::UnsupportedConfiguration);
        assert!(error.message.contains("未修改系统代理"));
        assert!(api.writes.lock().unwrap().is_empty());
    }
}
