use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::contracts::{
    ProxyError, ProxyErrorCode, SectionResult, TerminalProxyInput, TerminalSnapshot, TerminalValues,
};
use crate::rules::{
    TERMINAL_VARIABLES, parse_shell_block, render_shell_block, sanitize_terminal_values,
    upsert_shell_block,
};

use super::command::{CommandRunner, ErrorStage, args, explain_command_error, run_text};

const ID: &str = "/usr/bin/id";
const DSCL: &str = "/usr/bin/dscl";
const LAUNCHCTL: &str = "/bin/launchctl";
static TEMP_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// 文件接口让托管区块编排可以在临时目录或内存 fake 上测试。
pub trait ShellFileStore: Send + Sync {
    fn read(&self, path: &Path) -> Result<Option<String>, ProxyError>;
    fn replace(&self, path: &Path, content: &str) -> Result<(), ProxyError>;
}

pub struct RealShellFileStore;

impl ShellFileStore for RealShellFileStore {
    fn read(&self, path: &Path) -> Result<Option<String>, ProxyError> {
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(shell_io_error(ErrorStage::Read, error)),
        };
        reject_unsafe_target(&metadata, ErrorStage::Read)?;
        let bytes = fs::read(path).map_err(|error| shell_io_error(ErrorStage::Read, error))?;
        String::from_utf8(bytes).map(Some).map_err(|_| {
            ProxyError::unsupported(
                "默认 Shell 配置不是有效 UTF-8；请先备份并转换为 UTF-8 后重新读取，本应用未修改该文件",
            )
        })
    }

    fn replace(&self, path: &Path, content: &str) -> Result<(), ProxyError> {
        let existing = match fs::symlink_metadata(path) {
            Ok(metadata) => {
                reject_unsafe_target(&metadata, ErrorStage::Persist)?;
                Some(metadata)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(shell_io_error(ErrorStage::Persist, error)),
        };
        let parent = path.parent().ok_or_else(|| {
            ProxyError::persist("无法确定默认 Shell 配置所在目录；请检查当前用户主目录后重试")
        })?;
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| {
                ProxyError::persist("默认 Shell 配置文件名无效；请检查当前用户主目录后重试")
            })?;

        // 临时文件与目标同目录，rename 才是同一文件系统内的原子替换；create_new 防止预置链接攻击。
        let mut last_exists_error = None;
        for _ in 0..16 {
            let sequence = TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let temporary = parent.join(format!(
                ".{file_name}.proxy-manager.{}.{}.tmp",
                std::process::id(),
                sequence
            ));
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
            {
                Ok(mut file) => {
                    let result = (|| -> io::Result<()> {
                        if let Some(metadata) = &existing {
                            fs::set_permissions(&temporary, metadata.permissions())?;
                        }
                        file.write_all(content.as_bytes())?;
                        file.sync_all()?;
                        fs::rename(&temporary, path)?;
                        Ok(())
                    })();
                    if let Err(error) = result {
                        let _ = fs::remove_file(&temporary);
                        return Err(shell_io_error(ErrorStage::Persist, error));
                    }
                    return Ok(());
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    last_exists_error = Some(error);
                }
                Err(error) => return Err(shell_io_error(ErrorStage::Persist, error)),
            }
        }
        Err(shell_io_error(
            ErrorStage::Persist,
            last_exists_error.unwrap_or_else(|| {
                io::Error::new(io::ErrorKind::AlreadyExists, "temporary file exists")
            }),
        ))
    }
}

fn reject_unsafe_target(metadata: &fs::Metadata, stage: ErrorStage) -> Result<(), ProxyError> {
    let message = if metadata.file_type().is_symlink() {
        "默认 Shell 配置是符号链接；为避免改写其他文件，本次操作已停止。请先将实际配置放回默认路径并移除符号链接，再重新操作"
    } else if !metadata.is_file() {
        "默认 Shell 配置路径不是普通文件；请先检查并恢复为普通 UTF-8 文件，再重新操作"
    } else {
        return Ok(());
    };
    Err(match stage {
        ErrorStage::Read => ProxyError::read(message),
        ErrorStage::Persist => ProxyError::persist(message),
    })
}

fn shell_io_error(stage: ErrorStage, error: io::Error) -> ProxyError {
    let permission_denied = error.kind() == io::ErrorKind::PermissionDenied;
    let action = match stage {
        ErrorStage::Read => "读取默认 Shell 配置",
        ErrorStage::Persist => "写入默认 Shell 配置",
    };
    let message = if permission_denied {
        format!(
            "没有权限{action}；请检查当前用户对该文件及所在目录的权限后重试，本应用不会提升权限"
        )
    } else {
        match stage {
            ErrorStage::Read => format!("{action}失败；请确认文件可读取且当前用户主目录有效后重试"),
            ErrorStage::Persist => format!(
                "{action}失败，未继续更新 launchd；请检查磁盘空间和目录权限，重新读取实际配置后再试"
            ),
        }
    };
    ProxyError::new(
        if permission_denied {
            ProxyErrorCode::PermissionDenied
        } else {
            match stage {
                ErrorStage::Read => ProxyErrorCode::ReadFailed,
                ErrorStage::Persist => ProxyErrorCode::PersistFailed,
            }
        },
        message,
    )
}

pub struct TerminalState {
    config_path: PathBuf,
    pub persistence_label: String,
    pub persisted: TerminalValues,
    pub launchd: TerminalValues,
}

pub fn read_terminal(
    commands: &dyn CommandRunner,
    files: &dyn ShellFileStore,
) -> Result<TerminalState, ProxyError> {
    let account = read_current_account(commands)?;
    let (config_path, persistence_label) = shell_config(&account)?;
    let content = files.read(&config_path)?.unwrap_or_default();
    let persisted = parse_shell_block(&content)?;
    let launchd = read_launchd(commands)?;
    Ok(TerminalState {
        config_path,
        persistence_label,
        persisted,
        launchd,
    })
}

struct UserAccount {
    home: PathBuf,
    shell: String,
}

fn read_current_account(commands: &dyn CommandRunner) -> Result<UserAccount, ProxyError> {
    let username = run_text(commands, ID, &args(&["-un"]))
        .map_err(|error| {
            explain_command_error(
                error,
                ErrorStage::Read,
                "读取当前用户名称",
                "请确认当前 macOS 用户会话有效后重新读取",
            )
        })?
        .trim()
        .to_owned();
    if username.is_empty() || username.contains('/') || username.chars().any(char::is_control) {
        return Err(ProxyError::read(
            "当前用户名称无效，无法查询默认 Shell；请检查 macOS 用户账户后重新读取",
        ));
    }
    let record = format!("/Users/{username}");
    let output = run_text(
        commands,
        DSCL,
        &args(&[".", "-read", &record, "NFSHomeDirectory", "UserShell"]),
    )
    .map_err(|error| {
        explain_command_error(
            error,
            ErrorStage::Read,
            "从当前用户数据库读取主目录和默认 Shell",
            "请确认当前用户记录完整后重新读取",
        )
    })?;
    let mut home = None;
    let mut shell = None;
    for line in output.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        match key.trim() {
            "NFSHomeDirectory" => home = Some(value.trim().to_owned()),
            "UserShell" => shell = Some(value.trim().to_owned()),
            _ => {}
        }
    }
    let home = home.filter(|value| !value.is_empty()).ok_or_else(|| {
        ProxyError::read("当前用户数据库未提供主目录；请补全 macOS 用户记录后重新读取")
    })?;
    let shell = shell.filter(|value| !value.is_empty()).ok_or_else(|| {
        ProxyError::read(
            "当前用户数据库未提供默认 Shell；请将默认 Shell 设为 zsh 或 bash 后重新读取",
        )
    })?;
    Ok(UserAccount {
        home: PathBuf::from(home),
        shell,
    })
}

fn shell_config(account: &UserAccount) -> Result<(PathBuf, String), ProxyError> {
    match account.shell.as_str() {
        "/bin/zsh" => Ok((
            account.home.join(".zshrc"),
            "默认 zsh 的 ~/.zshrc 托管区块".to_owned(),
        )),
        "/bin/bash" => Ok((
            account.home.join(".bash_profile"),
            "默认 bash 的 ~/.bash_profile 托管区块".to_owned(),
        )),
        shell => Err(ProxyError::unsupported(format!(
            "当前默认 Shell 为“{shell}”，本期仅支持 /bin/zsh 或 /bin/bash；请在 macOS 用户设置中改用受支持 Shell，或手动维护代理配置"
        ))),
    }
}

fn read_launchd(commands: &dyn CommandRunner) -> Result<TerminalValues, ProxyError> {
    let mut values = TerminalValues::default();
    for name in TERMINAL_VARIABLES {
        let output = run_text(commands, LAUNCHCTL, &args(&["getenv", name])).map_err(|error| {
            explain_command_error(
                error,
                ErrorStage::Read,
                &format!("读取 launchd 当前会话中的 {name}"),
                "请确认当前用户 launchd 会话可用后重新读取",
            )
        })?;
        let value = output.trim_end_matches(['\r', '\n']).to_owned();
        set_value(&mut values, name, (!value.is_empty()).then_some(value));
    }
    Ok(values)
}

pub fn process_environment() -> (TerminalValues, Vec<String>) {
    let mut values = TerminalValues::default();
    let mut warnings = Vec::new();
    for name in TERMINAL_VARIABLES {
        match std::env::var(name) {
            Ok(value) => set_value(&mut values, name, Some(value)),
            Err(std::env::VarError::NotPresent) => {}
            Err(std::env::VarError::NotUnicode(_)) => warnings.push(format!(
                "应用进程中的 {name} 不是有效 UTF-8，快照未展示该值；请在新终端中检查并修正该环境变量"
            )),
        }
    }
    (values, warnings)
}

pub fn to_snapshot(state: &TerminalState) -> TerminalSnapshot {
    let (process, mut warnings) = process_environment();
    for (source, values) in [
        ("应用进程", &process),
        ("Shell 托管区块", &state.persisted),
        ("launchd 当前会话", &state.launchd),
    ] {
        if has_authenticated_proxy(values) {
            warnings.push(format!(
                "{source}中存在认证代理，已脱敏展示；本期不支持认证代理，请移除账号密码后再保存对应变量"
            ));
        }
    }
    TerminalSnapshot {
        process: sanitize_terminal_values(&process),
        persisted: sanitize_terminal_values(&state.persisted),
        persistence_label: state.persistence_label.clone(),
        launchd: Some(sanitize_terminal_values(&state.launchd)),
        warnings,
    }
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

pub fn apply_terminal(
    commands: &dyn CommandRunner,
    files: &dyn ShellFileStore,
    input: &TerminalProxyInput,
) -> Result<SectionResult, ProxyError> {
    let before = read_terminal(commands, files)?;
    let desired_persisted = merge_values(&before.persisted, input);
    let desired_launchd = merge_values(&before.launchd, input);
    let shell_changed = desired_persisted != before.persisted;
    let launchd_changed = desired_launchd != before.launchd;
    if !shell_changed && !launchd_changed {
        return Ok(SectionResult::unchanged(
            "Shell 托管区块与 launchd 当前会话均已符合请求，未重复写入；launchd 配置不跨注销持久化",
        ));
    }

    if shell_changed {
        let content = files.read(&before.config_path)?.unwrap_or_default();
        let updated = upsert_shell_block(&content, &render_shell_block(&desired_persisted))?;
        files.replace(&before.config_path, &updated)?;
    }
    for name in TERMINAL_VARIABLES {
        let previous = value(&before.launchd, name);
        let desired = value(&desired_launchd, name);
        if previous == desired {
            continue;
        }
        let (operation, arguments) = match desired {
            Some(value) => (
                "setenv",
                vec!["setenv".to_owned(), name.to_owned(), value.to_owned()],
            ),
            None => ("unsetenv", vec!["unsetenv".to_owned(), name.to_owned()]),
        };
        run_text(commands, LAUNCHCTL, &arguments).map_err(|error| {
            explain_command_error(
                error,
                ErrorStage::Persist,
                &format!("通过 launchctl {operation} 更新 {name}"),
                "请检查当前用户 launchd 会话权限，重新读取 Shell 与 launchd 的实际值后再决定是否需要重试；已完成的写入不会自动回滚",
            )
        })?;
    }

    let after = read_terminal(commands, files)?;
    if after.persisted != desired_persisted {
        return Err(ProxyError::verify(
            "Shell 托管区块保存后的回读结果与请求不一致；请不要直接重复保存，先检查该文件是否被其他程序修改，再从最新快照恢复草稿",
        ));
    }
    if after.launchd != desired_launchd {
        return Err(ProxyError::verify(
            "launchd 当前会话保存后的回读结果与请求不一致；请确认没有其他程序同时修改代理环境，重新读取后再逐项核对",
        ));
    }
    Ok(SectionResult::applied(
        "终端代理已按请求更新并回读一致；新终端或重新加载 Shell 配置后生效，launchd 配置不跨注销持久化",
    ))
}

fn merge_values(current: &TerminalValues, input: &TerminalProxyInput) -> TerminalValues {
    TerminalValues {
        http_proxy: merge_value(&current.http_proxy, &input.http_proxy),
        https_proxy: merge_value(&current.https_proxy, &input.https_proxy),
        all_proxy: merge_value(&current.all_proxy, &input.all_proxy),
        no_proxy: merge_value(&current.no_proxy, &input.no_proxy),
    }
}

fn merge_value(current: &Option<String>, input: &Option<String>) -> Option<String> {
    match input {
        None => current.clone(),
        Some(value) if value.is_empty() => None,
        Some(value) => Some(value.clone()),
    }
}

fn value<'a>(values: &'a TerminalValues, name: &str) -> &'a Option<String> {
    match name {
        "HTTP_PROXY" => &values.http_proxy,
        "HTTPS_PROXY" => &values.https_proxy,
        "ALL_PROXY" => &values.all_proxy,
        "NO_PROXY" => &values.no_proxy,
        _ => unreachable!("only known terminal variables are passed"),
    }
}

fn set_value(values: &mut TerminalValues, name: &str, new_value: Option<String>) {
    match name {
        "HTTP_PROXY" => values.http_proxy = new_value,
        "HTTPS_PROXY" => values.https_proxy = new_value,
        "ALL_PROXY" => values.all_proxy = new_value,
        "NO_PROXY" => values.no_proxy = new_value,
        _ => unreachable!("only known terminal variables are passed"),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Mutex;

    use super::*;
    use crate::platform::macos::command::CommandOutput;

    struct FakeCommands {
        replies: Mutex<VecDeque<io::Result<CommandOutput>>>,
        calls: Mutex<Vec<(String, Vec<String>)>>,
    }

    impl FakeCommands {
        fn new(replies: Vec<io::Result<CommandOutput>>) -> Self {
            Self {
                replies: Mutex::new(replies.into()),
                calls: Mutex::new(Vec::new()),
            }
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

    struct MemoryFiles {
        content: Mutex<Option<String>>,
        replacements: Mutex<usize>,
    }

    impl ShellFileStore for MemoryFiles {
        fn read(&self, _path: &Path) -> Result<Option<String>, ProxyError> {
            Ok(self.content.lock().unwrap().clone())
        }

        fn replace(&self, _path: &Path, content: &str) -> Result<(), ProxyError> {
            *self.content.lock().unwrap() = Some(content.to_owned());
            *self.replacements.lock().unwrap() += 1;
            Ok(())
        }
    }

    fn output(text: &str) -> io::Result<CommandOutput> {
        Ok(CommandOutput {
            status: 0,
            stdout: text.as_bytes().to_vec(),
            stderr: Vec::new(),
        })
    }

    fn account_replies() -> Vec<io::Result<CommandOutput>> {
        vec![
            output("tester\n"),
            output("NFSHomeDirectory: /Users/tester\nUserShell: /bin/zsh\n"),
        ]
    }

    #[test]
    fn apply_terminal_should_set_and_unset_only_changed_launchd_values() {
        let mut replies = account_replies();
        replies.extend([
            output(""),
            output("http://keep:2\n"),
            output(""),
            output("localhost\n"),
        ]);
        replies.extend([output(""), output("")]);
        replies.extend(account_replies());
        replies.extend([
            output("http://127.0.0.1:7890\n"),
            output("http://keep:2\n"),
            output(""),
            output(""),
        ]);
        let commands = FakeCommands::new(replies);
        let files = MemoryFiles {
            content: Mutex::new(Some("# unmanaged\n".to_owned())),
            replacements: Mutex::new(0),
        };
        let result = apply_terminal(
            &commands,
            &files,
            &TerminalProxyInput {
                http_proxy: Some("http://127.0.0.1:7890".to_owned()),
                https_proxy: None,
                all_proxy: None,
                no_proxy: Some(String::new()),
            },
        )
        .expect("terminal apply should succeed");
        assert!(result.message.contains("回读一致"));
        assert_eq!(*files.replacements.lock().unwrap(), 1);
        let saved = files.content.lock().unwrap().clone().unwrap_or_default();
        assert!(saved.starts_with("# unmanaged\n"));
        let values = parse_shell_block(&saved).unwrap();
        assert_eq!(values.http_proxy.as_deref(), Some("http://127.0.0.1:7890"));
        assert_eq!(values.no_proxy, None);
        let calls = commands.calls.lock().unwrap();
        assert!(calls.iter().any(|(program, args)| program == LAUNCHCTL
            && args
                == &vec![
                    "setenv".to_owned(),
                    "HTTP_PROXY".to_owned(),
                    "http://127.0.0.1:7890".to_owned(),
                ]));
        assert!(calls.iter().any(|(program, args)| program == LAUNCHCTL
            && args == &vec!["unsetenv".to_owned(), "NO_PROXY".to_owned()]));
        assert!(!calls.iter().any(
            |(_, args)| args.first().map(String::as_str) == Some("setenv")
                && args.get(1).map(String::as_str) == Some("HTTPS_PROXY")
        ));
    }

    #[test]
    fn merge_values_should_keep_delete_and_replace_requested_fields() {
        let current = TerminalValues {
            http_proxy: Some("http://old:1".to_owned()),
            https_proxy: Some("http://keep:2".to_owned()),
            all_proxy: None,
            no_proxy: Some("localhost".to_owned()),
        };
        let input = TerminalProxyInput {
            http_proxy: Some(String::new()),
            https_proxy: None,
            all_proxy: Some("socks5://127.0.0.1:3".to_owned()),
            no_proxy: None,
        };
        assert_eq!(
            merge_values(&current, &input),
            TerminalValues {
                http_proxy: None,
                https_proxy: Some("http://keep:2".to_owned()),
                all_proxy: Some("socks5://127.0.0.1:3".to_owned()),
                no_proxy: Some("localhost".to_owned()),
            }
        );
    }

    #[test]
    fn real_file_store_should_preserve_unmanaged_content() {
        let root = unique_test_directory("preserve");
        fs::create_dir_all(&root).expect("create test directory");
        let path = root.join(".zshrc");
        fs::write(&path, "# keep me\nalias ll='ls -l'\n").expect("write fixture");
        let store = RealShellFileStore;
        let original = store.read(&path).expect("read fixture").unwrap_or_default();
        let updated = upsert_shell_block(
            &original,
            &render_shell_block(&TerminalValues {
                http_proxy: Some("http://127.0.0.1:7890".to_owned()),
                ..TerminalValues::default()
            }),
        )
        .expect("render block");
        store.replace(&path, &updated).expect("atomic replacement");
        let saved = store.read(&path).expect("read saved").unwrap_or_default();
        assert!(saved.starts_with("# keep me\nalias ll='ls -l'\n"));
        assert_eq!(
            parse_shell_block(&saved).unwrap().http_proxy.as_deref(),
            Some("http://127.0.0.1:7890")
        );
        fs::remove_dir_all(root).expect("remove test directory");
    }

    #[cfg(unix)]
    #[test]
    fn real_file_store_should_reject_symlink_without_touching_target() {
        let root = unique_test_directory("symlink");
        fs::create_dir_all(&root).expect("create test directory");
        let target = root.join("actual");
        let link = root.join(".zshrc");
        fs::write(&target, "keep").expect("write fixture");
        std::os::unix::fs::symlink(&target, &link).expect("create symlink");
        let error = RealShellFileStore.replace(&link, "replace").unwrap_err();
        assert!(error.message.contains("符号链接"));
        assert_eq!(fs::read_to_string(&target).unwrap(), "keep");
        fs::remove_dir_all(root).expect("remove test directory");
    }

    fn unique_test_directory(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "proxy-manager-macos-test-{label}-{}-{}",
            std::process::id(),
            TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ))
    }
}
