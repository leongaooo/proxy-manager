use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export, export_to = "../../src/generated/")]
pub enum Platform {
    Windows,
    Macos,
    Unsupported,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../src/generated/")]
pub struct TerminalValues {
    pub http_proxy: Option<String>,
    pub https_proxy: Option<String>,
    pub all_proxy: Option<String>,
    pub no_proxy: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../src/generated/")]
pub struct TerminalSnapshot {
    /// 应用自己的进程环境，不代表其他已经运行的终端。
    pub process: TerminalValues,
    /// Windows 用户环境或 macOS 托管 Shell 区块中的持久值。
    pub persisted: TerminalValues,
    pub persistence_label: String,
    /// 仅 macOS 返回当前 launchd 用户会话中的值。
    pub launchd: Option<TerminalValues>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[ts(export, export_to = "../../src/generated/")]
pub struct ProxyEndpoint {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../src/generated/")]
pub struct SystemSnapshot {
    pub enabled: bool,
    pub http: Option<ProxyEndpoint>,
    pub https: Option<ProxyEndpoint>,
    pub socks: Option<ProxyEndpoint>,
    pub bypass: Vec<String>,
    pub available_targets: Vec<String>,
    pub selected_target: Option<String>,
    /// 只说明存在不兼容的自动配置，不在保存时擅自清除它。
    pub auto_config: Option<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../src/generated/")]
pub struct ProxySnapshot {
    pub platform: Platform,
    pub terminal: TerminalSnapshot,
    pub system: SystemSnapshot,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[ts(export, export_to = "../../src/generated/")]
pub struct TerminalProxyInput {
    /// None 表示保持该变量；Some("") 表示删除该变量。
    pub http_proxy: Option<String>,
    pub https_proxy: Option<String>,
    pub all_proxy: Option<String>,
    pub no_proxy: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[ts(export, export_to = "../../src/generated/")]
pub struct SystemProxyInput {
    /// 禁用时只关闭系统代理，保留已有服务器与绕过配置。
    pub enabled: bool,
    pub http: Option<ProxyEndpoint>,
    pub https: Option<ProxyEndpoint>,
    pub socks: Option<ProxyEndpoint>,
    pub bypass: Vec<String>,
    /// macOS 网络服务名称；Windows 忽略此字段。
    pub target: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[ts(export, export_to = "../../src/generated/")]
pub struct ApplyRequest {
    /// None 表示整个区块不参与本次保存。
    pub terminal: Option<TerminalProxyInput>,
    pub system: Option<SystemProxyInput>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export, export_to = "../../src/generated/")]
pub enum ApplyStatus {
    Applied,
    Unchanged,
    Partial,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export, export_to = "../../src/generated/")]
pub enum SectionStatus {
    Applied,
    Unchanged,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[ts(export, export_to = "../../src/generated/")]
pub enum ProxyErrorCode {
    ValidationFailed,
    PermissionDenied,
    TargetRequired,
    UnsupportedConfiguration,
    PersistFailed,
    NotifyFailed,
    VerifyFailed,
    ReadFailed,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../src/generated/")]
pub struct SectionResult {
    pub status: SectionStatus,
    pub code: Option<ProxyErrorCode>,
    pub message: String,
}

impl SectionResult {
    pub fn applied(message: impl Into<String>) -> Self {
        Self {
            status: SectionStatus::Applied,
            code: None,
            message: message.into(),
        }
    }

    pub fn unchanged(message: impl Into<String>) -> Self {
        Self {
            status: SectionStatus::Unchanged,
            code: None,
            message: message.into(),
        }
    }

    pub fn failed(error: ProxyError) -> Self {
        Self {
            status: SectionStatus::Failed,
            code: Some(error.code),
            message: error.message,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../src/generated/")]
pub struct ApplyReport {
    pub status: ApplyStatus,
    pub terminal: Option<SectionResult>,
    pub system: Option<SectionResult>,
    pub snapshot: ProxySnapshot,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyError {
    pub code: ProxyErrorCode,
    pub message: String,
}

impl ProxyError {
    pub fn new(code: ProxyErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn validation(message: impl Into<String>) -> Self {
        Self::new(ProxyErrorCode::ValidationFailed, message)
    }

    pub fn read(message: impl Into<String>) -> Self {
        Self::new(ProxyErrorCode::ReadFailed, message)
    }

    pub fn persist(message: impl Into<String>) -> Self {
        Self::new(ProxyErrorCode::PersistFailed, message)
    }

    pub fn verify(message: impl Into<String>) -> Self {
        Self::new(ProxyErrorCode::VerifyFailed, message)
    }

    pub fn unsupported(message: impl Into<String>) -> Self {
        Self::new(ProxyErrorCode::UnsupportedConfiguration, message)
    }
}
