use std::io;
use std::process::Command;

use crate::contracts::{ProxyError, ProxyErrorCode};

/// 命令边界只接收可执行文件和独立 argv，调用方不能传入 Shell 脚本。
pub trait CommandRunner: Send + Sync {
    fn run(&self, program: &str, args: &[String]) -> io::Result<CommandOutput>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    pub status: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

pub struct RealCommandRunner;

impl CommandRunner for RealCommandRunner {
    fn run(&self, program: &str, args: &[String]) -> io::Result<CommandOutput> {
        let output = Command::new(program).args(args).output()?;
        Ok(CommandOutput {
            status: output.status.code().unwrap_or(-1),
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }
}

#[derive(Debug)]
pub enum CommandError {
    Spawn(io::Error),
    Failed { permission_denied: bool },
    NonUtf8,
}

#[derive(Debug, Clone, Copy)]
pub enum ErrorStage {
    Read,
    Persist,
}

pub fn run_text(
    commands: &dyn CommandRunner,
    program: &str,
    args: &[String],
) -> Result<String, CommandError> {
    let output = commands.run(program, args).map_err(CommandError::Spawn)?;
    if output.status != 0 {
        let stderr = String::from_utf8_lossy(&output.stderr).to_ascii_lowercase();
        let permission_denied = [
            "permission denied",
            "operation not permitted",
            "not authorized",
            "authorization denied",
            "requires root",
            "administrator privileges",
        ]
        .iter()
        .any(|marker| stderr.contains(marker));
        return Err(CommandError::Failed { permission_denied });
    }
    String::from_utf8(output.stdout).map_err(|_| CommandError::NonUtf8)
}

/// 不向用户拼接命令行或标准错误，只公开当前动作、权限结论和恢复动作。
pub fn explain_command_error(
    error: CommandError,
    stage: ErrorStage,
    action: &str,
    recovery: &str,
) -> ProxyError {
    let permission_denied = match &error {
        CommandError::Spawn(error) => error.kind() == io::ErrorKind::PermissionDenied,
        CommandError::Failed { permission_denied } => *permission_denied,
        CommandError::NonUtf8 => false,
    };
    let code = if permission_denied {
        ProxyErrorCode::PermissionDenied
    } else {
        match stage {
            ErrorStage::Read => ProxyErrorCode::ReadFailed,
            ErrorStage::Persist => ProxyErrorCode::PersistFailed,
        }
    };
    let message = match error {
        CommandError::NonUtf8 => {
            format!("{action}返回了无法识别的非 UTF-8 数据；{recovery}，本应用未采用猜测值继续操作")
        }
        _ if permission_denied => format!("没有权限{action}；{recovery}"),
        CommandError::Spawn(_) => format!("无法启动系统功能以{action}；{recovery}"),
        CommandError::Failed { .. } => format!("{action}未执行成功；{recovery}"),
    };
    ProxyError::new(code, message)
}

pub fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explain_command_error_should_hide_stderr_and_offer_recovery() {
        let error = explain_command_error(
            CommandError::Failed {
                permission_denied: true,
            },
            ErrorStage::Persist,
            "设置 HTTP 系统代理",
            "请使用具备网络设置权限的管理员账户重试",
        );
        assert_eq!(error.code, ProxyErrorCode::PermissionDenied);
        assert!(error.message.contains("设置 HTTP 系统代理"));
        assert!(error.message.contains("管理员账户"));
        assert!(!error.message.contains("stderr"));
    }
}
