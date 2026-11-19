use std::collections::HashSet;

use url::{Host, Url};

use crate::contracts::{
    ProxyEndpoint, ProxyError, SystemProxyInput, TerminalProxyInput, TerminalValues,
};

pub const TERMINAL_VARIABLES: [&str; 4] = ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "NO_PROXY"];
pub const SHELL_BLOCK_BEGIN: &str = "# >>> proxy-manager managed proxy >>>";
pub const SHELL_BLOCK_END: &str = "# <<< proxy-manager managed proxy <<<";

/// 校验并归一化所有输入；此函数不读取或修改操作系统状态。
pub fn normalize_terminal(input: &TerminalProxyInput) -> Result<TerminalProxyInput, ProxyError> {
    Ok(TerminalProxyInput {
        http_proxy: normalize_proxy_field("HTTP_PROXY", &input.http_proxy, false)?,
        https_proxy: normalize_proxy_field("HTTPS_PROXY", &input.https_proxy, false)?,
        all_proxy: normalize_proxy_field("ALL_PROXY", &input.all_proxy, true)?,
        no_proxy: normalize_no_proxy(&input.no_proxy)?,
    })
}

pub fn normalize_system(input: &SystemProxyInput) -> Result<SystemProxyInput, ProxyError> {
    let target = input
        .target
        .as_ref()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());

    // 禁用只负责关闭开关，其他服务器值由平台原样保留。
    if !input.enabled {
        return Ok(SystemProxyInput {
            enabled: false,
            http: None,
            https: None,
            socks: None,
            bypass: Vec::new(),
            target,
        });
    }

    let normalized = SystemProxyInput {
        enabled: true,
        http: normalize_endpoint(input.http.as_ref(), "HTTP")?,
        https: normalize_endpoint(input.https.as_ref(), "HTTPS")?,
        socks: normalize_endpoint(input.socks.as_ref(), "SOCKS")?,
        bypass: normalize_bypass(&input.bypass)?,
        target,
    };

    if normalized.http.is_none() && normalized.https.is_none() && normalized.socks.is_none() {
        return Err(ProxyError::validation(
            "启用系统代理前，请至少填写 HTTP、HTTPS 或 SOCKS 中的一项服务器地址",
        ));
    }
    Ok(normalized)
}

fn normalize_proxy_field(
    name: &str,
    value: &Option<String>,
    allow_socks: bool,
) -> Result<Option<String>, ProxyError> {
    let Some(raw) = value else {
        return Ok(None);
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(Some(String::new()));
    }
    if trimmed.chars().any(char::is_control) {
        return Err(ProxyError::validation(format!(
            "{name} 含控制字符，请移除后重试"
        )));
    }

    let url = Url::parse(trimmed).map_err(|_| {
        ProxyError::validation(format!(
            "{name} 不是有效代理 URL，请输入例如 http://127.0.0.1:7890"
        ))
    })?;
    let scheme = url.scheme().to_ascii_lowercase();
    let scheme_valid = matches!(scheme.as_str(), "http" | "https")
        || (allow_socks && matches!(scheme.as_str(), "socks5" | "socks5h"));
    if !scheme_valid {
        return Err(ProxyError::validation(format!(
            "{name} 不支持 {scheme} 协议，请使用 {}协议",
            if allow_socks {
                "HTTP、HTTPS 或 SOCKS5"
            } else {
                "HTTP 或 HTTPS"
            }
        )));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(ProxyError::validation(format!(
            "{name} 含代理账号或密码，本期不支持认证代理；请移除认证信息后重试"
        )));
    }
    if url.host().is_none() || url.port().is_none() {
        return Err(ProxyError::validation(format!(
            "{name} 缺少有效主机或端口，请补全后重试"
        )));
    }
    if !matches!(url.path(), "" | "/") || url.query().is_some() || url.fragment().is_some() {
        return Err(ProxyError::validation(format!(
            "{name} 只能包含协议、主机和端口，不应包含路径、查询或片段"
        )));
    }
    Ok(Some(trimmed.to_owned()))
}

fn normalize_no_proxy(value: &Option<String>) -> Result<Option<String>, ProxyError> {
    let Some(raw) = value else {
        return Ok(None);
    };
    if raw.trim().is_empty() {
        return Ok(Some(String::new()));
    }
    if raw.chars().any(char::is_control) {
        return Err(ProxyError::validation("NO_PROXY 含控制字符，请移除后重试"));
    }

    let mut seen = HashSet::new();
    let mut entries = Vec::new();
    for entry in raw.split(',').map(str::trim) {
        if entry.is_empty() {
            return Err(ProxyError::validation(
                "NO_PROXY 含空条目，请使用英文逗号分隔主机或域名",
            ));
        }
        if entry.chars().any(char::is_whitespace) {
            return Err(ProxyError::validation(format!(
                "NO_PROXY 条目“{entry}”含空白字符，请修正后重试"
            )));
        }
        if seen.insert(entry.to_owned()) {
            entries.push(entry.to_owned());
        }
    }
    Ok(Some(entries.join(",")))
}

fn normalize_endpoint(
    endpoint: Option<&ProxyEndpoint>,
    label: &str,
) -> Result<Option<ProxyEndpoint>, ProxyError> {
    let Some(endpoint) = endpoint else {
        return Ok(None);
    };
    let host = endpoint.host.trim();
    if host.is_empty() {
        return Err(ProxyError::validation(format!(
            "{label} 系统代理主机不能为空"
        )));
    }
    if endpoint.port == 0 {
        return Err(ProxyError::validation(format!(
            "{label} 系统代理端口必须大于 0"
        )));
    }
    if host.chars().any(char::is_whitespace) || host.chars().any(char::is_control) {
        return Err(ProxyError::validation(format!(
            "{label} 系统代理主机含空白或控制字符，请修正后重试"
        )));
    }
    if host.contains("://") || host.contains('/') || host.contains('@') {
        return Err(ProxyError::validation(format!(
            "{label} 系统代理主机只填写主机名或 IP，不包含协议、路径或认证信息"
        )));
    }

    let host_for_parse = host
        .strip_prefix('[')
        .and_then(|v| v.strip_suffix(']'))
        .unwrap_or(host);
    let normalized_host = Host::parse(host_for_parse)
        .map_err(|_| ProxyError::validation(format!("{label} 系统代理主机“{host}”格式无效")))?;
    Ok(Some(ProxyEndpoint {
        host: match normalized_host {
            Host::Domain(value) => value,
            Host::Ipv4(value) => value.to_string(),
            Host::Ipv6(value) => value.to_string(),
        },
        port: endpoint.port,
    }))
}

fn normalize_bypass(entries: &[String]) -> Result<Vec<String>, ProxyError> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();
    for raw in entries {
        let entry = raw.trim();
        if entry.is_empty() {
            continue;
        }
        if entry.chars().any(char::is_control) || entry.chars().any(char::is_whitespace) {
            return Err(ProxyError::validation(format!(
                "代理绕过条目“{entry}”含空白或控制字符，请修正后重试"
            )));
        }
        if seen.insert(entry.to_owned()) {
            normalized.push(entry.to_owned());
        }
    }
    Ok(normalized)
}

pub fn format_endpoint(endpoint: &ProxyEndpoint) -> String {
    if endpoint.host.contains(':') {
        format!("[{}]:{}", endpoint.host, endpoint.port)
    } else {
        format!("{}:{}", endpoint.host, endpoint.port)
    }
}

pub fn parse_endpoint(raw: &str) -> Result<ProxyEndpoint, ProxyError> {
    let trimmed = raw.trim();
    let with_scheme = if trimmed.contains("://") {
        trimmed.to_owned()
    } else {
        format!("http://{trimmed}")
    };
    let url = Url::parse(&with_scheme).map_err(|_| {
        ProxyError::unsupported("已有系统代理服务器地址无法解析，请先在系统设置中修正")
    })?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err(ProxyError::unsupported(
            "已有系统代理使用认证信息，本期不会读取或覆盖该配置",
        ));
    }
    let host = url
        .host()
        .ok_or_else(|| ProxyError::unsupported("已有系统代理缺少主机"))?;
    let port = url
        .port_or_known_default()
        .ok_or_else(|| ProxyError::unsupported("已有系统代理缺少端口"))?;
    Ok(ProxyEndpoint {
        host: match host {
            Host::Domain(value) => value.to_owned(),
            Host::Ipv4(value) => value.to_string(),
            Host::Ipv6(value) => value.to_string(),
        },
        port,
    })
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct WindowsProxyServer {
    pub http: Option<ProxyEndpoint>,
    pub https: Option<ProxyEndpoint>,
    pub socks: Option<ProxyEndpoint>,
}

pub fn parse_windows_proxy_server(raw: &str) -> Result<WindowsProxyServer, ProxyError> {
    if raw.trim().is_empty() {
        return Ok(WindowsProxyServer::default());
    }

    let tokens: Vec<&str> = raw
        .split(';')
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .collect();
    if tokens.len() == 1 && !tokens[0].contains('=') {
        let endpoint = parse_endpoint(tokens[0])?;
        return Ok(WindowsProxyServer {
            http: Some(endpoint.clone()),
            https: Some(endpoint),
            socks: None,
        });
    }

    let mut result = WindowsProxyServer::default();
    for token in tokens {
        let (protocol, value) = token.split_once('=').ok_or_else(|| {
            ProxyError::unsupported(
                "已有 Windows 系统代理混用单服务器和分协议格式，请先核对系统设置",
            )
        })?;
        let endpoint = parse_endpoint(value)?;
        match protocol.trim().to_ascii_lowercase().as_str() {
            "http" if result.http.is_none() => result.http = Some(endpoint),
            "https" if result.https.is_none() => result.https = Some(endpoint),
            "socks" if result.socks.is_none() => result.socks = Some(endpoint),
            "http" | "https" | "socks" => {
                return Err(ProxyError::unsupported(
                    "已有 Windows 系统代理含重复协议配置",
                ));
            }
            _ => {
                return Err(ProxyError::unsupported(format!(
                    "已有 Windows 系统代理包含本期不支持的协议“{}”",
                    protocol.trim()
                )));
            }
        }
    }
    Ok(result)
}

pub fn format_windows_proxy_server(input: &SystemProxyInput) -> String {
    if input.socks.is_none()
        && let Some(endpoint) = &input.http
        && input.http == input.https
    {
        return format_endpoint(endpoint);
    }

    let mut entries = Vec::new();
    if let Some(endpoint) = &input.http {
        entries.push(format!("http={}", format_endpoint(endpoint)));
    }
    if let Some(endpoint) = &input.https {
        entries.push(format!("https={}", format_endpoint(endpoint)));
    }
    if let Some(endpoint) = &input.socks {
        entries.push(format!("socks={}", format_endpoint(endpoint)));
    }
    entries.join(";")
}

/// 已在操作系统中存在的认证代理只可脱敏展示，不能将掩码作为新值保存。
pub fn sanitize_terminal_values(values: &TerminalValues) -> TerminalValues {
    TerminalValues {
        http_proxy: values
            .http_proxy
            .as_ref()
            .map(|value| mask_proxy_url(value)),
        https_proxy: values
            .https_proxy
            .as_ref()
            .map(|value| mask_proxy_url(value)),
        all_proxy: values.all_proxy.as_ref().map(|value| mask_proxy_url(value)),
        no_proxy: values.no_proxy.clone(),
    }
}

fn mask_proxy_url(value: &str) -> String {
    let Ok(mut url) = Url::parse(value) else {
        return if value.contains('@') {
            "<已隐藏认证代理>".to_owned()
        } else {
            value.to_owned()
        };
    };
    if url.username().is_empty() && url.password().is_none() {
        return value.to_owned();
    }
    let _ = url.set_username("");
    let _ = url.set_password(None);
    url.to_string()
}

pub fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

pub fn render_shell_block(values: &TerminalValues) -> String {
    let entries = [
        ("HTTP_PROXY", &values.http_proxy),
        ("HTTPS_PROXY", &values.https_proxy),
        ("ALL_PROXY", &values.all_proxy),
        ("NO_PROXY", &values.no_proxy),
    ];
    let mut lines = vec![SHELL_BLOCK_BEGIN.to_owned()];
    for (name, value) in entries {
        lines.push(match value {
            Some(value) => format!("export {name}={}", shell_quote(value)),
            None => format!("unset {name}"),
        });
    }
    lines.push(SHELL_BLOCK_END.to_owned());
    lines.join("\n")
}

pub fn upsert_shell_block(content: &str, block: &str) -> Result<String, ProxyError> {
    let begins: Vec<_> = content.match_indices(SHELL_BLOCK_BEGIN).collect();
    let ends: Vec<_> = content.match_indices(SHELL_BLOCK_END).collect();
    if begins.len() > 1 || ends.len() > 1 || begins.len() != ends.len() {
        return Err(ProxyError::unsupported(
            "Shell 配置中的 proxy-manager 托管标记重复或残缺，请先手动整理该区块",
        ));
    }
    if begins.is_empty() {
        let mut result = content.to_owned();
        if !result.is_empty() && !result.ends_with('\n') {
            result.push('\n');
        }
        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str(block);
        result.push('\n');
        return Ok(result);
    }

    let begin = begins[0].0;
    let end = ends[0].0;
    if end < begin {
        return Err(ProxyError::unsupported(
            "Shell 配置中的 proxy-manager 托管标记顺序异常，请先手动整理该区块",
        ));
    }
    let mut result = content.to_owned();
    result.replace_range(begin..end + SHELL_BLOCK_END.len(), block);
    Ok(result)
}

pub fn parse_shell_block(content: &str) -> Result<TerminalValues, ProxyError> {
    if !content.contains(SHELL_BLOCK_BEGIN) && !content.contains(SHELL_BLOCK_END) {
        return Ok(TerminalValues::default());
    }
    let begins: Vec<_> = content.match_indices(SHELL_BLOCK_BEGIN).collect();
    let ends: Vec<_> = content.match_indices(SHELL_BLOCK_END).collect();
    if begins.len() != 1 || ends.len() != 1 || ends[0].0 < begins[0].0 {
        return Err(ProxyError::unsupported(
            "Shell 配置中的 proxy-manager 托管标记重复或残缺，请先手动整理该区块",
        ));
    }
    let block = &content[begins[0].0 + SHELL_BLOCK_BEGIN.len()..ends[0].0];
    let mut values = TerminalValues::default();
    for line in block.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let mut matched = false;
        for name in TERMINAL_VARIABLES {
            if line == format!("unset {name}") {
                set_terminal_value(&mut values, name, None);
                matched = true;
                break;
            }
            let prefix = format!("export {name}=");
            if let Some(quoted) = line.strip_prefix(&prefix) {
                let value = shell_unquote(quoted)?;
                set_terminal_value(&mut values, name, Some(value));
                matched = true;
                break;
            }
        }
        if !matched {
            return Err(ProxyError::unsupported(
                "Shell 托管区块含非本应用生成的内容，请先手动整理该区块",
            ));
        }
    }
    Ok(values)
}

fn shell_unquote(value: &str) -> Result<String, ProxyError> {
    if !value.starts_with('\'') || !value.ends_with('\'') {
        return Err(ProxyError::unsupported("Shell 托管区块中的值未正确引用"));
    }
    let inner = &value[1..value.len() - 1];
    if inner.contains('\'') && !inner.contains("'\"'\"'") {
        return Err(ProxyError::unsupported("Shell 托管区块中的引号格式异常"));
    }
    Ok(inner.replace("'\"'\"'", "'"))
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkService {
    pub name: String,
    pub device: Option<String>,
    pub disabled: bool,
}

pub fn parse_network_services(output: &str, disabled_output: &str) -> Vec<NetworkService> {
    let disabled_names: HashSet<String> = disabled_output
        .lines()
        .map(str::trim)
        .filter_map(|line| line.strip_prefix('*').map(str::trim))
        .map(ToOwned::to_owned)
        .collect();
    let mut services = Vec::new();
    let mut lines = output.lines().peekable();
    while let Some(line) = lines.next() {
        let trimmed = line.trim();
        if !trimmed.starts_with('(') {
            continue;
        }
        let Some((_, raw_name)) = trimmed.split_once(") ") else {
            continue;
        };
        let (marker_disabled, name) = raw_name
            .strip_prefix('*')
            .map_or((false, raw_name), |name| (true, name));
        let device = lines.next().and_then(|hardware| {
            hardware
                .split_once("Device: ")
                .map(|(_, value)| value.trim_end_matches(')').trim().to_owned())
        });
        services.push(NetworkService {
            name: name.to_owned(),
            device,
            disabled: marker_disabled || disabled_names.contains(name),
        });
    }
    services
}

pub fn parse_default_route_interface(output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        line.trim()
            .strip_prefix("interface:")
            .map(|value| value.trim().to_owned())
    })
}

pub fn choose_network_service(services: &[NetworkService], device: Option<&str>) -> Option<String> {
    let device = device?;
    let mut matches = services
        .iter()
        .filter(|service| !service.disabled && service.device.as_deref() == Some(device));
    let selected = matches.next()?;
    if matches.next().is_some() {
        None
    } else {
        Some(selected.name.clone())
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct NetworkProxyState {
    pub enabled: bool,
    pub endpoint: Option<ProxyEndpoint>,
    pub authenticated: bool,
}

pub fn parse_network_proxy_state(output: &str) -> Result<NetworkProxyState, ProxyError> {
    let mut enabled = false;
    let mut host = None;
    let mut port = None;
    let mut authenticated = false;
    for line in output.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        match key.trim() {
            "Enabled" => enabled = value.trim().eq_ignore_ascii_case("yes"),
            "Server" => host = Some(value.trim().to_owned()),
            "Port" => port = value.trim().parse::<u16>().ok(),
            "Authenticated Proxy Enabled" => {
                authenticated = matches!(value.trim(), "1" | "Yes" | "yes")
            }
            _ => {}
        }
    }
    let endpoint = match (host, port) {
        (Some(host), Some(port)) if !host.is_empty() && port > 0 => {
            Some(ProxyEndpoint { host, port })
        }
        _ => None,
    };
    Ok(NetworkProxyState {
        enabled,
        endpoint,
        authenticated,
    })
}

pub fn parse_network_bypass(output: &str) -> Vec<String> {
    output
        .lines()
        .map(str::trim)
        .filter(|line| {
            !line.is_empty()
                && !line.starts_with("There aren't any bypass domains")
                && !line.starts_with("Proxy Bypass Domains:")
        })
        .map(ToOwned::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_terminal_should_preserve_deletion_and_deduplicate_no_proxy() {
        let normalized = normalize_terminal(&TerminalProxyInput {
            http_proxy: Some(" http://127.0.0.1:7890 ".to_owned()),
            https_proxy: Some(String::new()),
            all_proxy: None,
            no_proxy: Some(" localhost,127.0.0.1,localhost ".to_owned()),
        })
        .unwrap();
        assert_eq!(
            normalized.http_proxy.as_deref(),
            Some("http://127.0.0.1:7890")
        );
        assert_eq!(normalized.https_proxy.as_deref(), Some(""));
        assert_eq!(normalized.all_proxy, None);
        assert_eq!(normalized.no_proxy.as_deref(), Some("localhost,127.0.0.1"));
    }

    #[test]
    fn normalize_terminal_should_reject_credentials_and_missing_port() {
        for (value, expected) in [
            ("http://user:secret@127.0.0.1:7890", "认证代理"),
            ("http://127.0.0.1", "主机或端口"),
            ("socks5://127.0.0.1:1080", "不支持"),
        ] {
            let error = normalize_terminal(&TerminalProxyInput {
                http_proxy: Some(value.to_owned()),
                ..TerminalProxyInput::default()
            })
            .unwrap_err();
            assert!(error.message.contains(expected));
            assert!(!error.message.contains("secret"));
        }
    }

    #[test]
    fn windows_proxy_server_should_round_trip_ipv6_and_protocols() {
        let input = SystemProxyInput {
            enabled: true,
            http: Some(ProxyEndpoint {
                host: "::1".to_owned(),
                port: 7890,
            }),
            https: Some(ProxyEndpoint {
                host: "proxy.local".to_owned(),
                port: 8443,
            }),
            socks: None,
            bypass: Vec::new(),
            target: None,
        };
        let formatted = format_windows_proxy_server(&input);
        assert_eq!(formatted, "http=[::1]:7890;https=proxy.local:8443");
        let parsed = parse_windows_proxy_server(&formatted).unwrap();
        assert_eq!(parsed.http, input.http);
        assert_eq!(parsed.https, input.https);
    }

    #[test]
    fn shell_block_should_preserve_user_content_and_escape_without_execution() {
        let original = "export KEEP=yes\nalias ll='ls -l'\n";
        let values = TerminalValues {
            http_proxy: Some("http://host:7890/'$(touch should-not-run)".to_owned()),
            ..TerminalValues::default()
        };
        let block = render_shell_block(&values);
        assert!(!block.contains("\nexport INJECTED="));
        assert!(block.contains("'\"'\"'"));
        let updated = upsert_shell_block(original, &block).unwrap();
        assert!(updated.starts_with(original));
        assert_eq!(parse_shell_block(&updated).unwrap(), values);
        let replaced =
            upsert_shell_block(&updated, &render_shell_block(&TerminalValues::default())).unwrap();
        assert_eq!(replaced.match_indices(SHELL_BLOCK_BEGIN).count(), 1);
        assert!(replaced.contains("export KEEP=yes"));
    }

    #[test]
    fn shell_block_should_reject_broken_markers() {
        let error = parse_shell_block(SHELL_BLOCK_BEGIN).unwrap_err();
        assert!(error.message.contains("重复或残缺"));
    }

    #[test]
    fn sanitize_terminal_values_should_remove_credentials_from_snapshot() {
        let values = sanitize_terminal_values(&TerminalValues {
            http_proxy: Some("http://user:secret@127.0.0.1:7890".to_owned()),
            ..TerminalValues::default()
        });
        let visible = values.http_proxy.unwrap();
        assert!(visible.contains("127.0.0.1:7890"));
        assert!(!visible.contains("user"));
        assert!(!visible.contains("secret"));
    }

    #[test]
    fn network_parsers_should_select_unique_service_and_parse_disabled_state() {
        let order = "(1) Wi-Fi 工作\n(Hardware Port: Wi-Fi, Device: en0)\n(2) VPN\n(Hardware Port: VPN, Device: utun9)\n";
        let all = "* VPN\nWi-Fi 工作\n";
        let services = parse_network_services(order, all);
        assert!(services[1].disabled);
        assert_eq!(
            choose_network_service(&services, Some("en0")).as_deref(),
            Some("Wi-Fi 工作")
        );
        assert_eq!(choose_network_service(&services, Some("en8")), None);
        let state = parse_network_proxy_state(
            "Enabled: No\nServer: 127.0.0.1\nPort: 7890\nAuthenticated Proxy Enabled: 0\n",
        )
        .unwrap();
        assert!(!state.enabled);
        assert_eq!(state.endpoint.unwrap().port, 7890);
    }
}
