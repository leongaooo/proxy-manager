use crate::contracts::{
    ProxyEndpoint, ProxyError, ProxyErrorCode, SectionResult, SystemProxyInput, SystemSnapshot,
};
use crate::rules::{
    NetworkProxyState, NetworkService, choose_network_service, parse_default_route_interface,
    parse_network_bypass, parse_network_proxy_state, parse_network_services,
};

use super::command::{CommandRunner, ErrorStage, args, explain_command_error, run_text};

const NETWORKSETUP: &str = "/usr/sbin/networksetup";
const ROUTE: &str = "/sbin/route";

struct LoadedSystem {
    snapshot: SystemSnapshot,
    selected: Option<SelectedService>,
}

struct SelectedService {
    name: String,
    state: ServiceProxyState,
}

struct ServiceProxyState {
    http: NetworkProxyState,
    https: NetworkProxyState,
    socks: NetworkProxyState,
    bypass: Vec<String>,
    auto_config: Option<String>,
}

pub fn read_system(
    commands: &dyn CommandRunner,
    target: Option<&str>,
) -> Result<SystemSnapshot, ProxyError> {
    Ok(load_system(commands, target, false)?.snapshot)
}

pub fn apply_system(
    commands: &dyn CommandRunner,
    input: &SystemProxyInput,
) -> Result<SectionResult, ProxyError> {
    let loaded = load_system(commands, input.target.as_deref(), true)?;
    let selected = loaded.selected.ok_or_else(|| {
        ProxyError::new(
            ProxyErrorCode::TargetRequired,
            "请选择要修改的活动网络服务后重试",
        )
    })?;
    let target = selected.name;
    let before = selected.state;

    // 禁用是降低代理作用范围的安全操作，不因已有 PAC 或认证配置而阻止关闭手动代理。
    if input.enabled {
        reject_auto_configuration(&target, &before)?;
        reject_authenticated_proxies(&target, &before)?;
    }

    if !system_changes_required(input, &before) {
        return Ok(SectionResult::unchanged(if input.enabled {
            format!("网络服务“{target}”的系统代理已符合请求，未重复写入")
        } else {
            format!(
                "网络服务“{target}”的 HTTP、HTTPS 和 SOCKS 手动代理均已关闭，服务器与绕过配置保持原样"
            )
        }));
    }

    if input.enabled {
        apply_protocol(
            commands,
            &target,
            Protocol::Http,
            input.http.as_ref(),
            &before.http,
        )?;
        apply_protocol(
            commands,
            &target,
            Protocol::Https,
            input.https.as_ref(),
            &before.https,
        )?;
        apply_protocol(
            commands,
            &target,
            Protocol::Socks,
            input.socks.as_ref(),
            &before.socks,
        )?;
        if input.bypass != before.bypass {
            let mut arguments = vec!["-setproxybypassdomains".to_owned(), target.clone()];
            if input.bypass.is_empty() {
                arguments.push("Empty".to_owned());
            } else {
                arguments.extend(input.bypass.iter().cloned());
            }
            run_network_write(commands, &arguments, "更新代理绕过列表", &target)?;
        }
    } else {
        disable_protocol(commands, &target, Protocol::Http, &before.http)?;
        disable_protocol(commands, &target, Protocol::Https, &before.https)?;
        disable_protocol(commands, &target, Protocol::Socks, &before.socks)?;
    }

    let after = read_service(commands, &target)?;
    verify_system(input, &before, &after, &target)?;
    Ok(SectionResult::applied(if input.enabled {
        format!("网络服务“{target}”的系统代理已更新并回读一致；保存成功不代表代理服务器可连接")
    } else {
        format!("网络服务“{target}”的手动系统代理已关闭并回读一致，服务器与绕过配置保持原样")
    }))
}

fn load_system(
    commands: &dyn CommandRunner,
    target: Option<&str>,
    target_required: bool,
) -> Result<LoadedSystem, ProxyError> {
    let (services, route_device) = list_services(commands, target)?;
    let available_targets: Vec<String> = services
        .iter()
        .filter(|service| !service.disabled)
        .map(|service| service.name.clone())
        .collect();
    let selected_name = match select_service(&services, route_device.as_deref(), target) {
        Ok(name) => name,
        Err(error) if !target_required && error.code == ProxyErrorCode::TargetRequired => {
            return Ok(LoadedSystem {
                snapshot: SystemSnapshot {
                    enabled: false,
                    http: None,
                    https: None,
                    socks: None,
                    bypass: Vec::new(),
                    available_targets,
                    selected_target: None,
                    auto_config: None,
                    warnings: vec![error.message],
                },
                selected: None,
            });
        }
        Err(error) => return Err(error),
    };

    let state = read_service(commands, &selected_name)?;
    let mut warnings = Vec::new();
    if let Some(auto_config) = &state.auto_config {
        warnings.push(format!(
            "网络服务“{selected_name}”已启用{auto_config}，与手动代理可能冲突；保存前请在系统设置中关闭该自动配置，本应用不会擅自清除"
        ));
    }
    let authenticated = authenticated_protocols(&state);
    if !authenticated.is_empty() {
        warnings.push(format!(
            "网络服务“{selected_name}”的{}代理启用了认证，本期不支持认证代理；请在系统设置中移除认证配置后再保存",
            authenticated.join("、")
        ));
    }
    let snapshot = SystemSnapshot {
        enabled: state.http.enabled || state.https.enabled || state.socks.enabled,
        http: state.http.endpoint.clone(),
        https: state.https.endpoint.clone(),
        socks: state.socks.endpoint.clone(),
        bypass: state.bypass.clone(),
        available_targets,
        selected_target: Some(selected_name.clone()),
        auto_config: state.auto_config.clone(),
        warnings,
    };
    Ok(LoadedSystem {
        snapshot,
        selected: Some(SelectedService {
            name: selected_name,
            state,
        }),
    })
}

fn list_services(
    commands: &dyn CommandRunner,
    target: Option<&str>,
) -> Result<(Vec<NetworkService>, Option<String>), ProxyError> {
    let order = run_network_read(
        commands,
        &args(&["-listnetworkserviceorder"]),
        "读取网络服务与接口对应关系",
    )?;
    let all = run_network_read(
        commands,
        &args(&["-listallnetworkservices"]),
        "读取网络服务列表",
    )?;
    let services = parse_network_services(&order, &all);
    if services.is_empty() {
        return Err(ProxyError::read(
            "未读取到任何 macOS 网络服务；请确认网络服务已配置后重新读取",
        ));
    }
    if target.is_some() {
        return Ok((services, None));
    }
    let route = run_text(commands, ROUTE, &args(&["-n", "get", "default"])).map_err(|error| {
        explain_command_error(
            error,
            ErrorStage::Read,
            "读取默认路由接口",
            "请检查网络连接，或在系统代理区块明确选择目标网络服务",
        )
    })?;
    Ok((services, parse_default_route_interface(&route)))
}

fn select_service(
    services: &[NetworkService],
    route_device: Option<&str>,
    target: Option<&str>,
) -> Result<String, ProxyError> {
    if let Some(target) = target {
        return services
            .iter()
            .find(|service| service.name == target && !service.disabled)
            .map(|service| service.name.clone())
            .ok_or_else(|| {
                ProxyError::validation(format!(
                    "网络服务“{target}”不存在或已停用；请重新读取并从可用服务列表中选择"
                ))
            });
    }
    choose_network_service(services, route_device).ok_or_else(|| {
        let detail = match route_device {
            None => "默认路由未提供接口",
            Some(device) => {
                let count = services
                    .iter()
                    .filter(|service| {
                        !service.disabled && service.device.as_deref() == Some(device)
                    })
                    .count();
                if count > 1 {
                    "默认路由接口对应多个活动网络服务"
                } else {
                    "默认路由接口没有对应的唯一活动网络服务"
                }
            }
        };
        ProxyError::new(
            ProxyErrorCode::TargetRequired,
            format!("{detail}，无法自动选择；请在系统代理区块明确选择目标网络服务后重试"),
        )
    })
}

fn read_service(
    commands: &dyn CommandRunner,
    target: &str,
) -> Result<ServiceProxyState, ProxyError> {
    let auto_config = read_auto_configuration(commands, target)?;
    let http = read_protocol(commands, target, Protocol::Http)?;
    let https = read_protocol(commands, target, Protocol::Https)?;
    let socks = read_protocol(commands, target, Protocol::Socks)?;
    let bypass_output = run_network_read(
        commands,
        &args(&["-getproxybypassdomains", target]),
        "读取代理绕过列表",
    )?;
    Ok(ServiceProxyState {
        http,
        https,
        socks,
        bypass: parse_network_bypass(&bypass_output),
        auto_config,
    })
}

fn read_auto_configuration(
    commands: &dyn CommandRunner,
    target: &str,
) -> Result<Option<String>, ProxyError> {
    let pac = run_network_read(
        commands,
        &args(&["-getautoproxyurl", target]),
        "读取自动代理配置（PAC）",
    )?;
    let discovery = run_network_read(
        commands,
        &args(&["-getproxyautodiscovery", target]),
        "读取自动代理发现（WPAD）",
    )?;
    let mut enabled = Vec::new();
    if proxy_feature_enabled(&pac) {
        enabled.push("自动代理配置（PAC）");
    }
    if proxy_feature_enabled(&discovery) {
        enabled.push("自动代理发现（WPAD）");
    }
    Ok((!enabled.is_empty()).then(|| enabled.join("和")))
}

fn proxy_feature_enabled(output: &str) -> bool {
    output.lines().any(|line| {
        line.split_once(':').is_some_and(|(key, value)| {
            key.trim() == "Enabled" && value.trim().eq_ignore_ascii_case("yes")
        })
    })
}

fn read_protocol(
    commands: &dyn CommandRunner,
    target: &str,
    protocol: Protocol,
) -> Result<NetworkProxyState, ProxyError> {
    let output = run_network_read(
        commands,
        &args(&[protocol.get_command(), target]),
        &format!("读取{}代理", protocol.label()),
    )?;
    parse_network_proxy_state(&output)
}

fn run_network_read(
    commands: &dyn CommandRunner,
    arguments: &[String],
    action: &str,
) -> Result<String, ProxyError> {
    run_text(commands, NETWORKSETUP, arguments).map_err(|error| {
        explain_command_error(
            error,
            ErrorStage::Read,
            action,
            "请确认目标网络服务仍存在且当前用户可读取网络设置，然后重新读取",
        )
    })
}

fn run_network_write(
    commands: &dyn CommandRunner,
    arguments: &[String],
    action: &str,
    target: &str,
) -> Result<(), ProxyError> {
    run_text(commands, NETWORKSETUP, arguments)
        .map(|_| ())
        .map_err(|error| {
            explain_command_error(
                error,
                ErrorStage::Persist,
                action,
                &format!(
                    "请确认当前账户具备网络设置权限且网络服务“{target}”仍存在；本应用不会调用 sudo。此前项目可能已写入，请重新读取实际配置后再决定下一步"
                ),
            )
        })
}

#[derive(Clone, Copy)]
enum Protocol {
    Http,
    Https,
    Socks,
}

impl Protocol {
    fn label(self) -> &'static str {
        match self {
            Self::Http => "HTTP",
            Self::Https => "HTTPS",
            Self::Socks => "SOCKS",
        }
    }

    fn get_command(self) -> &'static str {
        match self {
            Self::Http => "-getwebproxy",
            Self::Https => "-getsecurewebproxy",
            Self::Socks => "-getsocksfirewallproxy",
        }
    }

    fn set_command(self) -> &'static str {
        match self {
            Self::Http => "-setwebproxy",
            Self::Https => "-setsecurewebproxy",
            Self::Socks => "-setsocksfirewallproxy",
        }
    }

    fn state_command(self) -> &'static str {
        match self {
            Self::Http => "-setwebproxystate",
            Self::Https => "-setsecurewebproxystate",
            Self::Socks => "-setsocksfirewallproxystate",
        }
    }
}

fn apply_protocol(
    commands: &dyn CommandRunner,
    target: &str,
    protocol: Protocol,
    desired: Option<&ProxyEndpoint>,
    current: &NetworkProxyState,
) -> Result<(), ProxyError> {
    match desired {
        Some(endpoint) => {
            if current.enabled && current.endpoint.as_ref() == Some(endpoint) {
                return Ok(());
            }
            let arguments = vec![
                protocol.set_command().to_owned(),
                target.to_owned(),
                endpoint.host.clone(),
                endpoint.port.to_string(),
            ];
            run_network_write(
                commands,
                &arguments,
                &format!("设置{}代理服务器", protocol.label()),
                target,
            )
        }
        None => disable_protocol(commands, target, protocol, current),
    }
}

fn disable_protocol(
    commands: &dyn CommandRunner,
    target: &str,
    protocol: Protocol,
    current: &NetworkProxyState,
) -> Result<(), ProxyError> {
    if !current.enabled {
        return Ok(());
    }
    run_network_write(
        commands,
        &args(&[protocol.state_command(), target, "off"]),
        &format!("关闭{}手动代理", protocol.label()),
        target,
    )
}

fn system_changes_required(input: &SystemProxyInput, current: &ServiceProxyState) -> bool {
    if !input.enabled {
        return current.http.enabled || current.https.enabled || current.socks.enabled;
    }
    protocol_changes_required(input.http.as_ref(), &current.http)
        || protocol_changes_required(input.https.as_ref(), &current.https)
        || protocol_changes_required(input.socks.as_ref(), &current.socks)
        || input.bypass != current.bypass
}

fn protocol_changes_required(desired: Option<&ProxyEndpoint>, current: &NetworkProxyState) -> bool {
    match desired {
        Some(endpoint) => !current.enabled || current.endpoint.as_ref() != Some(endpoint),
        None => current.enabled,
    }
}

fn reject_auto_configuration(target: &str, state: &ServiceProxyState) -> Result<(), ProxyError> {
    if let Some(auto_config) = &state.auto_config {
        return Err(ProxyError::unsupported(format!(
            "网络服务“{target}”已启用{auto_config}，与手动系统代理冲突；请先在 macOS 系统设置 → 网络 → 详细信息 → 代理中关闭该自动配置，再重新读取并保存。本应用未修改任何网络配置"
        )));
    }
    Ok(())
}

fn reject_authenticated_proxies(target: &str, state: &ServiceProxyState) -> Result<(), ProxyError> {
    let protocols = authenticated_protocols(state);
    if protocols.is_empty() {
        return Ok(());
    }
    Err(ProxyError::unsupported(format!(
        "网络服务“{target}”的{}代理启用了认证，本期不支持认证代理；请先在系统设置中移除账号密码配置，再重新读取并保存。本应用未读取或覆盖凭据，也未修改网络配置",
        protocols.join("、")
    )))
}

fn authenticated_protocols(state: &ServiceProxyState) -> Vec<&'static str> {
    [
        (Protocol::Http, &state.http),
        (Protocol::Https, &state.https),
        (Protocol::Socks, &state.socks),
    ]
    .into_iter()
    .filter(|(_, proxy)| proxy.authenticated)
    .map(|(protocol, _)| protocol.label())
    .collect()
}

fn verify_system(
    input: &SystemProxyInput,
    before: &ServiceProxyState,
    after: &ServiceProxyState,
    target: &str,
) -> Result<(), ProxyError> {
    if input.enabled {
        verify_protocol(
            input.http.as_ref(),
            &before.http,
            &after.http,
            Protocol::Http,
            target,
        )?;
        verify_protocol(
            input.https.as_ref(),
            &before.https,
            &after.https,
            Protocol::Https,
            target,
        )?;
        verify_protocol(
            input.socks.as_ref(),
            &before.socks,
            &after.socks,
            Protocol::Socks,
            target,
        )?;
        if after.bypass != input.bypass {
            return Err(verify_error(target, "代理绕过列表"));
        }
    } else {
        for (protocol, before_protocol, after_protocol) in [
            (Protocol::Http, &before.http, &after.http),
            (Protocol::Https, &before.https, &after.https),
            (Protocol::Socks, &before.socks, &after.socks),
        ] {
            if after_protocol.enabled || after_protocol.endpoint != before_protocol.endpoint {
                return Err(verify_error(target, protocol.label()));
            }
        }
        if after.bypass != before.bypass {
            return Err(verify_error(target, "代理绕过列表"));
        }
    }
    Ok(())
}

fn verify_protocol(
    desired: Option<&ProxyEndpoint>,
    before: &NetworkProxyState,
    after: &NetworkProxyState,
    protocol: Protocol,
    target: &str,
) -> Result<(), ProxyError> {
    let matches = match desired {
        Some(endpoint) => after.enabled && after.endpoint.as_ref() == Some(endpoint),
        None => !after.enabled && after.endpoint == before.endpoint,
    };
    if matches {
        Ok(())
    } else {
        Err(verify_error(target, protocol.label()))
    }
}

fn verify_error(target: &str, field: &str) -> ProxyError {
    ProxyError::verify(format!(
        "网络服务“{target}”的{field}保存后回读与请求不一致；请检查是否有其他程序同时修改网络设置，重新读取实际值后再决定是否需要重试，本应用不会盲目覆盖新配置"
    ))
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::io;
    use std::sync::Mutex;

    use super::*;
    use crate::platform::macos::command::CommandOutput;

    struct FakeCommands {
        replies: Mutex<VecDeque<io::Result<CommandOutput>>>,
        pub calls: Mutex<Vec<(String, Vec<String>)>>,
    }

    impl FakeCommands {
        fn new(replies: Vec<io::Result<CommandOutput>>) -> Self {
            Self {
                replies: Mutex::new(replies.into()),
                calls: Mutex::new(Vec::new()),
            }
        }

        fn assert_finished(&self) {
            assert!(
                self.replies.lock().unwrap().is_empty(),
                "存在未消费的命令回包"
            );
        }
    }

    impl CommandRunner for FakeCommands {
        fn run(&self, program: &str, args: &[String]) -> io::Result<CommandOutput> {
            self.calls
                .lock()
                .unwrap()
                .push((program.to_owned(), args.to_vec()));
            self.replies
                .lock()
                .unwrap()
                .pop_front()
                .expect("unexpected command")
        }
    }

    fn output(text: &str) -> io::Result<CommandOutput> {
        Ok(CommandOutput {
            status: 0,
            stdout: text.as_bytes().to_vec(),
            stderr: Vec::new(),
        })
    }

    fn disabled_proxy() -> io::Result<CommandOutput> {
        output("Enabled: No\nServer: \nPort: 0\nAuthenticated Proxy Enabled: 0\n")
    }

    fn service_replies(http: &str, bypass: &str) -> Vec<io::Result<CommandOutput>> {
        vec![
            output("URL: \nEnabled: No\n"),
            output("Enabled: No\n"),
            output(http),
            disabled_proxy(),
            disabled_proxy(),
            output(bypass),
        ]
    }

    fn selection_replies() -> Vec<io::Result<CommandOutput>> {
        vec![
            output(
                "An asterisk (*) denotes that a network service is disabled.\n(1) Wi-Fi 办公\n(Hardware Port: Wi-Fi, Device: en0)\n",
            ),
            output("An asterisk (*) denotes that a network service is disabled.\nWi-Fi 办公\n"),
            output("interface: en0\n"),
        ]
    }

    #[test]
    fn apply_system_should_use_argv_and_verify_without_shell_or_sudo() {
        let mut replies = selection_replies();
        replies.extend(service_replies(
            "Enabled: No\nServer: \nPort: 0\nAuthenticated Proxy Enabled: 0\n",
            "old.local\n",
        ));
        replies.push(output(""));
        replies.push(output(""));
        replies.extend(service_replies(
            "Enabled: Yes\nServer: 127.0.0.1\nPort: 7890\nAuthenticated Proxy Enabled: 0\n",
            "localhost\n",
        ));
        let commands = FakeCommands::new(replies);
        let input = SystemProxyInput {
            enabled: true,
            http: Some(ProxyEndpoint {
                host: "127.0.0.1".to_owned(),
                port: 7890,
            }),
            https: None,
            socks: None,
            bypass: vec!["localhost".to_owned()],
            target: None,
        };
        let result = apply_system(&commands, &input).expect("apply should succeed");
        assert!(result.message.contains("回读一致"));
        commands.assert_finished();
        let calls = commands.calls.lock().unwrap();
        assert!(
            calls
                .iter()
                .all(|(program, _)| program != "sudo" && program != "/bin/sh")
        );
        assert!(calls.iter().any(|(_, args)| args
            == &vec![
                "-setwebproxy".to_owned(),
                "Wi-Fi 办公".to_owned(),
                "127.0.0.1".to_owned(),
                "7890".to_owned(),
            ]));
        assert!(calls.iter().any(|(_, args)| args
            == &vec![
                "-setproxybypassdomains".to_owned(),
                "Wi-Fi 办公".to_owned(),
                "localhost".to_owned(),
            ]));
    }

    #[test]
    fn apply_system_should_only_disable_and_preserve_server_and_bypass() {
        let mut replies = selection_replies();
        replies.extend(service_replies(
            "Enabled: Yes\nServer: proxy.local\nPort: 8080\nAuthenticated Proxy Enabled: 0\n",
            "localhost\n",
        ));
        replies.push(output(""));
        replies.extend(service_replies(
            "Enabled: No\nServer: proxy.local\nPort: 8080\nAuthenticated Proxy Enabled: 0\n",
            "localhost\n",
        ));
        let commands = FakeCommands::new(replies);
        let result = apply_system(
            &commands,
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
        assert!(result.message.contains("保持原样"));
        commands.assert_finished();
        let calls = commands.calls.lock().unwrap();
        let writes: Vec<_> = calls
            .iter()
            .filter(|(_, args)| args[0].starts_with("-set"))
            .collect();
        assert_eq!(writes.len(), 1);
        assert_eq!(
            writes[0].1,
            vec![
                "-setwebproxystate".to_owned(),
                "Wi-Fi 办公".to_owned(),
                "off".to_owned(),
            ]
        );
    }

    #[test]
    fn apply_system_should_require_target_when_route_is_ambiguous() {
        let commands = FakeCommands::new(vec![
            output(
                "(1) Wi-Fi A\n(Hardware Port: Wi-Fi, Device: en0)\n(2) Wi-Fi B\n(Hardware Port: Wi-Fi, Device: en0)\n",
            ),
            output("Wi-Fi A\nWi-Fi B\n"),
            output("interface: en0\n"),
        ]);
        let error = apply_system(
            &commands,
            &SystemProxyInput {
                enabled: false,
                http: None,
                https: None,
                socks: None,
                bypass: Vec::new(),
                target: None,
            },
        )
        .unwrap_err();
        assert_eq!(error.code, ProxyErrorCode::TargetRequired);
        assert!(error.message.contains("明确选择"));
        commands.assert_finished();
        assert_eq!(commands.calls.lock().unwrap().len(), 3);
    }

    #[test]
    fn apply_system_should_block_authenticated_proxy_before_any_write() {
        let mut replies = selection_replies();
        replies.extend(service_replies(
            "Enabled: Yes\nServer: proxy.local\nPort: 8080\nAuthenticated Proxy Enabled: 1\n",
            "localhost\n",
        ));
        let commands = FakeCommands::new(replies);
        let error = apply_system(
            &commands,
            &SystemProxyInput {
                enabled: true,
                http: Some(ProxyEndpoint {
                    host: "127.0.0.1".to_owned(),
                    port: 7890,
                }),
                https: None,
                socks: None,
                bypass: vec!["localhost".to_owned()],
                target: None,
            },
        )
        .unwrap_err();
        assert_eq!(error.code, ProxyErrorCode::UnsupportedConfiguration);
        assert!(error.message.contains("HTTP"));
        assert!(error.message.contains("未修改网络配置"));
        commands.assert_finished();
        assert!(
            commands
                .calls
                .lock()
                .unwrap()
                .iter()
                .all(|(_, args)| !args[0].starts_with("-set"))
        );
    }
}
