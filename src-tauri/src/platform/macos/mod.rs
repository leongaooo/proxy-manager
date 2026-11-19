mod command;
mod network;
mod shell;

use crate::contracts::{
    Platform, ProxyError, ProxySnapshot, SectionResult, SystemProxyInput, TerminalProxyInput,
};
use crate::platform::PlatformAdapter;

use command::RealCommandRunner;
use shell::RealShellFileStore;

/// 真实 macOS 适配器只在 platform/mod.rs 的 target_os = "macos" 条件下编译。
/// 所有系统副作用都经过命令与文件接口，测试不会读取或改写真实代理配置。
pub struct MacosAdapter;

impl PlatformAdapter for MacosAdapter {
    fn read_snapshot(&self, target: Option<&str>) -> Result<ProxySnapshot, ProxyError> {
        let commands = RealCommandRunner;
        let files = RealShellFileStore;
        let terminal_state = shell::read_terminal(&commands, &files)?;
        let terminal = shell::to_snapshot(&terminal_state);
        let system = network::read_system(&commands, target)?;
        Ok(ProxySnapshot {
            platform: Platform::Macos,
            terminal,
            system,
            warnings: Vec::new(),
        })
    }

    fn apply_terminal(&self, input: &TerminalProxyInput) -> Result<SectionResult, ProxyError> {
        let commands = RealCommandRunner;
        let files = RealShellFileStore;
        shell::apply_terminal(&commands, &files, input)
    }

    fn apply_system(&self, input: &SystemProxyInput) -> Result<SectionResult, ProxyError> {
        network::apply_system(&RealCommandRunner, input)
    }
}
