use crate::contracts::ProxyError;
use crate::contracts::{
    ApplyStatus, ProxySnapshot, SectionResult, SystemProxyInput, TerminalProxyInput,
};

// 测试构建同时编译 macOS 的纯编排和 fake 适配，便于在 Windows 验证平台中立规则；
// 真实入口仍只在 macOS 选择该适配器，不伪造 Apple Silicon 真机验证。
#[cfg(any(target_os = "macos", test))]
pub mod macos;
#[cfg(windows)]
pub mod windows;

/// 平台实现负责真实读写与回读；编排层只处理已归一化的数据。
pub trait PlatformAdapter: Send + Sync {
    fn read_snapshot(&self, target: Option<&str>) -> Result<ProxySnapshot, ProxyError>;

    fn apply_terminal(&self, input: &TerminalProxyInput) -> Result<SectionResult, ProxyError>;

    fn apply_system(&self, input: &SystemProxyInput) -> Result<SectionResult, ProxyError>;
}

pub fn current_adapter() -> Box<dyn PlatformAdapter> {
    #[cfg(windows)]
    {
        return Box::new(windows::WindowsAdapter);
    }
    #[cfg(target_os = "macos")]
    {
        return Box::new(macos::MacosAdapter);
    }
    #[allow(unreachable_code)]
    Box::new(UnsupportedAdapter)
}

struct UnsupportedAdapter;

impl PlatformAdapter for UnsupportedAdapter {
    fn read_snapshot(&self, _target: Option<&str>) -> Result<ProxySnapshot, ProxyError> {
        Err(ProxyError::unsupported(
            "当前操作系统不受支持，请使用 Windows 或 macOS",
        ))
    }

    fn apply_terminal(&self, _input: &TerminalProxyInput) -> Result<SectionResult, ProxyError> {
        Err(ProxyError::unsupported(
            "当前操作系统不受支持，请使用 Windows 或 macOS",
        ))
    }

    fn apply_system(&self, _input: &SystemProxyInput) -> Result<SectionResult, ProxyError> {
        Err(ProxyError::unsupported(
            "当前操作系统不受支持，请使用 Windows 或 macOS",
        ))
    }
}

pub fn combine_status(
    terminal: Option<&SectionResult>,
    system: Option<&SectionResult>,
) -> ApplyStatus {
    let results: Vec<_> = terminal.into_iter().chain(system).collect();
    if results.is_empty()
        || results
            .iter()
            .all(|result| result.status == crate::contracts::SectionStatus::Unchanged)
    {
        return ApplyStatus::Unchanged;
    }
    if results
        .iter()
        .any(|result| result.status == crate::contracts::SectionStatus::Failed)
    {
        return if results
            .iter()
            .all(|result| result.status == crate::contracts::SectionStatus::Failed)
        {
            ApplyStatus::Failed
        } else {
            ApplyStatus::Partial
        };
    }
    ApplyStatus::Applied
}
