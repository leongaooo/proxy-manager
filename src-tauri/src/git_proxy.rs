//! Git 全局配置是独立副作用边界，不读取或写入终端/系统代理。
//! 原始认证地址仅留在进程内；前端用快照身份请求保留值或进行乐观并发核对。
use std::collections::HashSet;
use std::io;
use std::process::{Command, Output};
use std::sync::Mutex;

use url::Url;

use crate::contracts::{
    ApplyStatus, GitApplyReport, GitProxyInput, GitProxyMode, GitProxySnapshot, ProxyError,
    ProxyErrorCode,
};

trait GitRunner: Send + Sync {
    fn run(&self, args: &[&str]) -> io::Result<Output>;
}

struct RealGitRunner;

impl GitRunner for RealGitRunner {
    fn run(&self, args: &[&str]) -> io::Result<Output> {
        let mut command = Command::new("git");
        command.args(args);
        // Git trace 可将 argv（含认证地址）写入文件；此边界不继承这些诊断出口。
        for variable in [
            "GIT_TRACE",
            "GIT_TRACE2",
            "GIT_TRACE2_EVENT",
            "GIT_TRACE2_PERF",
            "GIT_TRACE_SETUP",
            "GIT_TRACE_CURL",
            "GIT_CURL_VERBOSE",
        ] {
            command.env_remove(variable);
        }
        // GUI 应用读取配置不能闪出控制台；每个参数直接传给 Git，不经过 Shell。
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        command.output()
    }
}

#[derive(Clone, PartialEq, Eq)]
struct RawSnapshot {
    available: bool,
    direct: Vec<(String, String)>,
    effective: Vec<(String, String)>,
    keys: Vec<(String, String)>,
    write_target: String,
}

impl RawSnapshot {
    fn current_value(&self) -> Option<&str> {
        self.effective.last().map(|(_, value)| value.as_str())
    }

    fn warnings(&self) -> Vec<String> {
        let mut warnings = Vec::new();
        if !self.available {
            warnings.push("未找到 Git，请安装 Git 并重新启动应用后重试。".to_owned());
            return warnings;
        }
        if self.effective != self.direct {
            warnings
                .push("http.proxy 来自 include 配置，本页只读；请在原配置文件中核对。".to_owned());
        }
        if self.direct.len() > 1 {
            warnings.push(
                "存在多个全局 http.proxy，本页只读；请先手动整理重复键或多来源配置。".to_owned(),
            );
        }
        let origins: HashSet<_> = self.keys.iter().map(|(origin, _)| origin).collect();
        if origins.len() > 1 {
            warnings.push(
                "全局配置存在多个来源，写入目标可能不同于现有值；请先手动核对，本页只读。"
                    .to_owned(),
            );
        }
        if self.direct.len() == 1 && !self.direct_is_write_target() {
            warnings.push("现有 http.proxy 所在文件与 Git 全局写入目标不同，本页只读；请先核对 XDG 与用户配置。".to_owned());
        }
        if self.keys.iter().any(|(_, key)| key == "https.proxy") {
            warnings.push(
                "发现非规范 https.proxy，本应用保留该键；HTTP/HTTPS 共用 http.proxy。".to_owned(),
            );
        }
        warnings
    }

    fn writable(&self) -> bool {
        let origins: HashSet<_> = self.keys.iter().map(|(origin, _)| origin).collect();
        self.available
            && self.effective == self.direct
            && self.direct.len() <= 1
            && origins.len() <= 1
            && self.direct_is_write_target()
    }

    fn direct_is_write_target(&self) -> bool {
        self.direct.iter().all(|(origin, _)| {
            origin
                .strip_prefix("file:")
                .is_some_and(|path| comparable_path(path) == comparable_path(&self.write_target))
        })
    }

    fn public(&self, revision: String) -> GitProxySnapshot {
        let value = self.current_value();
        GitProxySnapshot {
            available: self.available,
            writable: self.writable(),
            mode: match value {
                None => GitProxyMode::Inherit,
                Some("") => GitProxyMode::Disabled,
                Some(_) => GitProxyMode::Custom,
            },
            proxy: value.map(mask_proxy),
            revision,
            warnings: self.warnings(),
        }
    }
}

#[derive(Default)]
struct ReadState {
    revision: u64,
    snapshot: Option<RawSnapshot>,
}

/// 串行化本应用的读写；外部编辑只能尽力进行保存前核对，不承诺跨进程原子事务。
pub struct GitProxyService {
    runner: Box<dyn GitRunner>,
    state: Mutex<ReadState>,
}

impl Default for GitProxyService {
    fn default() -> Self {
        Self {
            runner: Box::new(RealGitRunner),
            state: Mutex::new(ReadState::default()),
        }
    }
}

impl GitProxyService {
    pub fn read(&self) -> Result<GitProxySnapshot, ProxyError> {
        let mut state = self.state.lock().map_err(|_| poisoned())?;
        let raw = self.read_raw()?;
        Ok(publish(&mut state, raw))
    }

    pub fn apply(&self, input: &GitProxyInput) -> Result<GitApplyReport, ProxyError> {
        let mut state = self.state.lock().map_err(|_| poisoned())?;
        let before = self.read_raw()?;
        if state.snapshot.as_ref() != Some(&before)
            || input.expected_revision != state.revision.to_string()
        {
            return Ok(GitApplyReport {
                status: ApplyStatus::Failed,
                message: "Git 全局配置已变化，本次未写入；请重新读取后核对草稿。".to_owned(),
                snapshot: Some(publish(&mut state, before)),
            });
        }
        if !before.writable() {
            return Ok(GitApplyReport {
                status: ApplyStatus::Failed,
                message: "当前 Git 配置不可写，请先处理上方冲突或安装提示。".to_owned(),
                snapshot: Some(publish(&mut state, before)),
            });
        }
        let desired = desired_value(input, &before)?;
        if before.current_value() == desired.as_deref() {
            return Ok(GitApplyReport {
                status: ApplyStatus::Unchanged,
                message: "Git 全局代理无需更改，已核对当前配置。".to_owned(),
                snapshot: Some(publish(&mut state, before)),
            });
        }
        let args = match desired.as_deref() {
            Some(value) => vec!["config", "--global", "--replace-all", "http.proxy", value],
            None => vec!["config", "--global", "--unset-all", "http.proxy"],
        };
        let write_result = self.runner.run(&args);
        let write_error = match write_result {
            Ok(output) if output.status.success() => None,
            Ok(output) => Some(command_failure(&output, true)),
            Err(error) => Some(spawn_failure(error, true)),
        };
        // 即使命令失败也回读，不能用旧快照掩盖部分副作用或外部并发修改。
        let after = match self.read_raw() {
            Ok(after) => after,
            Err(_) => {
                state.snapshot = None;
                return Ok(GitApplyReport {
                    status: ApplyStatus::Partial,
                    message:
                        "保存已尝试，但回读失败，无法确认实际写入结果；草稿已保留，请重新读取。"
                            .to_owned(),
                    snapshot: None,
                });
            }
        };
        let matches = after.writable() && after.current_value() == desired.as_deref();
        let (status, message) = if let Some(error) = write_error {
            (ApplyStatus::Failed, error.message)
        } else if !matches {
            (
                ApplyStatus::Partial,
                "Git 写入命令已完成，但回读与目标不一致；可能存在并发修改，请重新读取核对。"
                    .to_owned(),
            )
        } else {
            (
                ApplyStatus::Applied,
                "Git 全局代理已保存并回读；尚未测试代理连通性。".to_owned(),
            )
        };
        Ok(GitApplyReport {
            status,
            message,
            snapshot: Some(publish(&mut state, after)),
        })
    }

    fn read_raw(&self) -> Result<RawSnapshot, ProxyError> {
        let paths = match self.global_paths() {
            Err(error) if error.code == ProxyErrorCode::UnsupportedConfiguration => {
                return Ok(RawSnapshot {
                    available: false,
                    direct: Vec::new(),
                    effective: Vec::new(),
                    keys: Vec::new(),
                    write_target: String::new(),
                });
            }
            result => result?,
        };
        let write_target = select_write_target(&paths)?;
        let mut raw = RawSnapshot {
            available: true,
            direct: Vec::new(),
            effective: Vec::new(),
            keys: Vec::new(),
            write_target,
        };
        // --global 在部分 Git 版本只读取写入目标；逐个读取 Git 公开的全局源，
        // 才能看见 XDG 被用户配置或空文件遮蔽的情况。绝不读取仓库/worktree 配置。
        for path in &paths {
            raw.direct.extend(self.read_pairs(&[
                "config",
                "--file",
                path,
                "--no-includes",
                "--null",
                "--show-origin",
                "--get-all",
                "http.proxy",
            ])?);
            raw.effective.extend(self.read_pairs(&[
                "config",
                "--file",
                path,
                "--includes",
                "--null",
                "--show-origin",
                "--get-all",
                "http.proxy",
            ])?);
            raw.keys.extend(self.read_pairs(&[
                "config",
                "--file",
                path,
                "--no-includes",
                "--null",
                "--show-origin",
                "--name-only",
                "--get-regexp",
                ".",
            ])?);
        }
        Ok(raw)
    }

    fn global_paths(&self) -> Result<Vec<String>, ProxyError> {
        // 由 Git 自己解析 HOME/XDG/GIT_CONFIG_GLOBAL，避免与 Git for Windows 的路径规则分叉。
        // 旧 Git 无法公开此变量时停止写入，不猜测目标位置。
        let output = self
            .runner
            .run(&["var", "GIT_CONFIG_GLOBAL"])
            .map_err(|error| spawn_failure(error, false))?;
        if !output.status.success() {
            return Err(ProxyError::read(
                "当前 Git 版本无法确认全局写入目标；请升级 Git 后重新读取，本次未写入。",
            ));
        }
        let text = String::from_utf8(output.stdout)
            .map_err(|_| ProxyError::read("Git 全局配置路径不是有效 UTF-8，请手动核对。"))?;
        let mut paths: Vec<_> = text
            .lines()
            .filter(|line| !line.is_empty())
            .map(ToOwned::to_owned)
            .collect();
        if paths.is_empty() || paths.len() > 2 {
            return Err(ProxyError::read(
                "Git 返回的全局配置路径不明确，请检查 Git 环境。",
            ));
        }
        // Git 的全局读取优先级是 XDG 在前、用户 .gitconfig 在后，不依赖 var 输出顺序。
        paths.sort_by_key(|path| is_user_config(path));
        paths.dedup();
        Ok(paths)
    }

    fn read_pairs(&self, args: &[&str]) -> Result<Vec<(String, String)>, ProxyError> {
        let output = self
            .runner
            .run(args)
            .map_err(|error| spawn_failure(error, false))?;
        if output.status.code() == Some(1) && output.stdout.is_empty() {
            return Ok(Vec::new());
        }
        if !output.status.success() {
            return Err(command_failure(&output, false));
        }
        let text = String::from_utf8(output.stdout).map_err(|_| {
            ProxyError::read("Git 配置不是有效 UTF-8，本次未采用猜测值；请先手动核对。")
        })?;
        let Some(content) = text.strip_suffix('\0') else {
            return Err(ProxyError::read(
                "Git 配置读取格式异常，请检查 Git 版本后重新读取。",
            ));
        };
        let parts: Vec<_> = content.split('\0').collect();
        if parts.len() % 2 != 0 {
            return Err(ProxyError::read(
                "Git 配置读取格式异常，请检查 Git 版本后重新读取。",
            ));
        }
        Ok(parts
            .chunks_exact(2)
            .map(|pair| (pair[0].to_owned(), pair[1].to_owned()))
            .collect())
    }
}

fn publish(state: &mut ReadState, raw: RawSnapshot) -> GitProxySnapshot {
    state.revision += 1;
    let snapshot = raw.public(state.revision.to_string());
    state.snapshot = Some(raw);
    snapshot
}

/// --global 写入现存用户 .gitconfig；否则写现存 XDG；两者都不存在则创建用户文件。
fn select_write_target(paths: &[String]) -> Result<String, ProxyError> {
    let mut target = paths
        .last()
        .cloned()
        .ok_or_else(|| ProxyError::read("Git 未返回全局配置路径，请检查 Git 环境。"))?;
    for path in paths {
        if std::path::Path::new(path)
            .try_exists()
            .map_err(|_| ProxyError::read("无法检查 Git 全局配置路径，请核对目录权限。"))?
        {
            target = path.clone();
        }
    }
    Ok(target)
}

fn is_user_config(path: &str) -> bool {
    std::path::Path::new(path)
        .file_name()
        .is_some_and(|name| name == ".gitconfig")
}

fn comparable_path(path: &str) -> String {
    let path = std::fs::canonicalize(path)
        .unwrap_or_else(|_| std::path::PathBuf::from(path))
        .to_string_lossy()
        .replace('\\', "/");
    #[cfg(windows)]
    let path = path.to_lowercase();
    path
}

fn desired_value(
    input: &GitProxyInput,
    before: &RawSnapshot,
) -> Result<Option<String>, ProxyError> {
    match input.mode {
        GitProxyMode::Inherit => Ok(None),
        GitProxyMode::Disabled => Ok(Some(String::new())),
        GitProxyMode::Custom => match &input.proxy {
            None => before
                .current_value()
                .filter(|value| !value.is_empty())
                .map(|value| Some(value.to_owned()))
                .ok_or_else(|| ProxyError::validation("请填写 Git 代理地址。")),
            Some(value) => {
                let trimmed = value.trim();
                if trimmed.is_empty() || trimmed.chars().any(char::is_control) {
                    return Err(ProxyError::validation(
                        "请填写有效 Git 代理地址，且不包含控制字符。",
                    ));
                }
                let parsed = Url::parse(trimmed).map_err(|_| {
                    ProxyError::validation("Git 代理地址格式无效，请填写完整 URL。")
                })?;
                if !matches!(
                    parsed.scheme(),
                    "http" | "https" | "socks4" | "socks4a" | "socks5" | "socks5h"
                ) || parsed.host().is_none()
                    || !matches!(parsed.path(), "" | "/")
                    || parsed.query().is_some()
                    || parsed.fragment().is_some()
                {
                    return Err(ProxyError::validation(
                        "Git 代理仅支持 HTTP、HTTPS 或 SOCKS 主机地址，不含路径、查询或片段。",
                    ));
                }
                Ok(Some(trimmed.to_owned()))
            }
        },
    }
}

fn mask_proxy(value: &str) -> String {
    let Ok(mut parsed) = Url::parse(value) else {
        return if value.contains('@') {
            "<已隐藏认证代理>".to_owned()
        } else {
            value.to_owned()
        };
    };
    if parsed.username().is_empty() && parsed.password().is_none() {
        return value.to_owned();
    }
    let _ = parsed.set_username("***");
    let _ = parsed.set_password(Some("***"));
    parsed.to_string()
}

fn poisoned() -> ProxyError {
    ProxyError::persist("Git 保存异常中断，请重启应用后重新读取实际配置。")
}

fn spawn_failure(error: io::Error, writing: bool) -> ProxyError {
    if error.kind() == io::ErrorKind::NotFound {
        return ProxyError::unsupported(
            "未找到 Git，请安装 Git 并重新启动应用。且本次未确认写入成功。",
        );
    }
    let code = if error.kind() == io::ErrorKind::PermissionDenied {
        ProxyErrorCode::PermissionDenied
    } else if writing {
        ProxyErrorCode::PersistFailed
    } else {
        ProxyErrorCode::ReadFailed
    };
    ProxyError::new(
        code,
        "无法运行 Git 配置命令，请检查 Git 安装和当前用户权限后重新读取。",
    )
}

/// stderr 仅用于固定错误分类，绝不把原文、路径、命令参数或认证地址返回给 UI/日志。
fn command_failure(output: &Output, writing: bool) -> ProxyError {
    let stderr = String::from_utf8_lossy(&output.stderr).to_ascii_lowercase();
    let (code, message) = if stderr.contains(".lock") || stderr.contains("could not lock") {
        (
            ProxyErrorCode::PersistFailed,
            "Git 配置锁被占用或不可写；请等待其他 Git 操作结束并检查目录权限后重新读取，勿直接删除活跃锁。",
        )
    } else if stderr.contains("permission denied")
        || stderr.contains("access is denied")
        || stderr.contains("read-only")
    {
        (
            ProxyErrorCode::PermissionDenied,
            "Git 全局配置不可写，请检查文件和目录权限后重新读取。",
        )
    } else if writing {
        (
            ProxyErrorCode::PersistFailed,
            "Git 配置写入未确认成功，请重新读取实际配置后重试。",
        )
    } else {
        (
            ProxyErrorCode::ReadFailed,
            "Git 全局配置读取失败，请检查配置语法、include 来源及文件权限后重试。",
        )
    };
    ProxyError::new(code, message)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

    use super::*;

    static NEXT_TEST: AtomicU64 = AtomicU64::new(0);

    struct Sandbox {
        root: PathBuf,
    }

    impl Sandbox {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "proxy-manager-git-{}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT_TEST.fetch_add(1, Ordering::Relaxed),
            ));
            fs::create_dir_all(&root).unwrap();
            Self { root }
        }

        fn config(&self) -> PathBuf {
            self.root.join("global.gitconfig")
        }

        fn write(&self, content: &str) {
            fs::write(self.config(), content).unwrap();
        }

        fn contents(&self) -> String {
            fs::read_to_string(self.config()).unwrap_or_default()
        }

        fn service(&self) -> GitProxyService {
            GitProxyService {
                runner: Box::new(IsolatedRunner {
                    root: self.root.clone(),
                    fail_readback: false,
                    pin_global: true,
                    wrote: AtomicBool::new(false),
                }),
                state: Mutex::new(ReadState::default()),
            }
        }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    /// 每条子进程显式隔离 HOME 与 Git 全局配置，不修改测试进程环境或真实用户文件。
    struct IsolatedRunner {
        root: PathBuf,
        fail_readback: bool,
        pin_global: bool,
        wrote: AtomicBool,
    }

    impl GitRunner for IsolatedRunner {
        fn run(&self, args: &[&str]) -> io::Result<Output> {
            let is_write = args.contains(&"--replace-all") || args.contains(&"--unset-all");
            if self.fail_readback && self.wrote.load(Ordering::Relaxed) && !is_write {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "secret must not escape",
                ));
            }
            let mut command = Command::new("git");
            command
                .args(args)
                .env("HOME", &self.root)
                .env("USERPROFILE", &self.root)
                .env("XDG_CONFIG_HOME", &self.root)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env_remove("GIT_CONFIG_COUNT")
                .env_remove("GIT_CONFIG_PARAMETERS")
                .env_remove("GIT_CONFIG")
                .current_dir(&self.root);
            if self.pin_global {
                command.env("GIT_CONFIG_GLOBAL", self.root.join("global.gitconfig"));
            } else {
                command.env_remove("GIT_CONFIG_GLOBAL");
            }
            let output = command.output();
            if is_write {
                self.wrote.store(true, Ordering::Relaxed);
            }
            output
        }
    }

    fn input(
        snapshot: &GitProxySnapshot,
        mode: GitProxyMode,
        proxy: Option<&str>,
    ) -> GitProxyInput {
        GitProxyInput {
            mode,
            proxy: proxy.map(ToOwned::to_owned),
            expected_revision: snapshot.revision.clone(),
        }
    }

    #[test]
    fn modes_are_distinct_and_preserve_unrelated_and_specific_keys() {
        let sandbox = Sandbox::new();
        sandbox.write("[user]\n name = Test\n[https]\n proxy = untouched\n[http \"https://example.com\"]\n proxy = specific\n[remote \"origin\"]\n proxy = remote\n");
        let service = sandbox.service();
        let first = service.read().unwrap();
        assert_eq!(first.mode, GitProxyMode::Inherit);
        assert!(
            first
                .warnings
                .iter()
                .any(|warning| warning.contains("https.proxy"))
        );
        let custom = service
            .apply(&input(
                &first,
                GitProxyMode::Custom,
                Some("http://127.0.0.1:7890"),
            ))
            .unwrap();
        assert_eq!(custom.status, ApplyStatus::Applied);
        let disabled = service
            .apply(&input(
                &custom.snapshot.unwrap(),
                GitProxyMode::Disabled,
                None,
            ))
            .unwrap();
        assert_eq!(disabled.status, ApplyStatus::Applied);
        assert_eq!(
            disabled.snapshot.as_ref().unwrap().mode,
            GitProxyMode::Disabled
        );
        assert_eq!(
            disabled.snapshot.as_ref().unwrap().proxy.as_deref(),
            Some("")
        );
        let inherit = service
            .apply(&input(
                &disabled.snapshot.unwrap(),
                GitProxyMode::Inherit,
                None,
            ))
            .unwrap();
        assert_eq!(inherit.status, ApplyStatus::Applied);
        assert_eq!(inherit.snapshot.unwrap().mode, GitProxyMode::Inherit);
        let contents = sandbox.contents();
        for preserved in [
            "name = Test",
            "proxy = untouched",
            "proxy = specific",
            "proxy = remote",
        ] {
            assert!(contents.contains(preserved));
        }
    }

    #[test]
    fn authentication_is_masked_and_unchanged_input_keeps_original_bytes() {
        let sandbox = Sandbox::new();
        sandbox.write("[http]\n proxy = http://alice:private-password@localhost:7890\n");
        let original = sandbox.contents();
        let service = sandbox.service();
        let snapshot = service.read().unwrap();
        let serialized = serde_json::to_string(&snapshot).unwrap();
        assert!(!serialized.contains("alice"));
        assert!(!serialized.contains("private-password"));
        let report = service
            .apply(&input(&snapshot, GitProxyMode::Custom, None))
            .unwrap();
        assert_eq!(report.status, ApplyStatus::Unchanged);
        assert_eq!(sandbox.contents(), original);
    }

    #[test]
    fn external_changes_are_rejected_without_writing() {
        let sandbox = Sandbox::new();
        let service = sandbox.service();
        let snapshot = service.read().unwrap();
        sandbox.write("[http]\n proxy = http://external:8000\n");
        let report = service
            .apply(&input(&snapshot, GitProxyMode::Disabled, None))
            .unwrap();
        assert_eq!(report.status, ApplyStatus::Failed);
        assert!(sandbox.contents().contains("http://external:8000"));
    }

    #[test]
    fn duplicate_keys_are_read_only_and_remain_intact() {
        let sandbox = Sandbox::new();
        sandbox.write("[http]\n proxy = http://one:8000\n proxy = http://two:8000\n");
        let original = sandbox.contents();
        let service = sandbox.service();
        let snapshot = service.read().unwrap();
        assert!(!snapshot.writable);
        let report = service
            .apply(&input(&snapshot, GitProxyMode::Disabled, None))
            .unwrap();
        assert_eq!(report.status, ApplyStatus::Failed);
        assert_eq!(sandbox.contents(), original);
    }

    #[test]
    fn included_proxy_is_read_only_and_not_removed_from_source() {
        let sandbox = Sandbox::new();
        fs::write(
            sandbox.root.join("included"),
            "[http]\n proxy = http://included:7890\n",
        )
        .unwrap();
        sandbox.write("[include]\n path = included\n");
        let service = sandbox.service();
        let snapshot = service.read().unwrap();
        assert_eq!(snapshot.mode, GitProxyMode::Custom);
        assert!(!snapshot.writable);
        let report = service
            .apply(&input(&snapshot, GitProxyMode::Inherit, None))
            .unwrap();
        assert_eq!(report.status, ApplyStatus::Failed);
        assert!(sandbox.contents().contains("path = included"));
        assert!(
            fs::read_to_string(sandbox.root.join("included"))
                .unwrap()
                .contains("http://included:7890")
        );
    }

    #[test]
    fn lock_failure_is_reported_without_disclosing_paths_or_credentials() {
        let sandbox = Sandbox::new();
        sandbox.write("[http]\n proxy = http://old:7890\n");
        fs::write(
            sandbox.root.join("global.gitconfig.lock"),
            "held by another process",
        )
        .unwrap();
        let service = sandbox.service();
        let snapshot = service.read().unwrap();
        let report = service
            .apply(&input(
                &snapshot,
                GitProxyMode::Custom,
                Some("http://alice:secret@new:7890"),
            ))
            .unwrap();
        assert_eq!(report.status, ApplyStatus::Failed);
        assert!(report.message.contains("锁"));
        let serialized = serde_json::to_string(&report).unwrap();
        assert!(!serialized.contains("alice"));
        assert!(!serialized.contains("secret"));
        assert!(!serialized.contains("gitconfig.lock"));
        assert!(sandbox.contents().contains("http://old:7890"));
    }

    #[test]
    fn write_followed_by_failed_readback_is_partial_and_keeps_no_stale_snapshot() {
        let sandbox = Sandbox::new();
        let service = GitProxyService {
            runner: Box::new(IsolatedRunner {
                root: sandbox.root.clone(),
                fail_readback: true,
                pin_global: true,
                wrote: AtomicBool::new(false),
            }),
            state: Mutex::new(ReadState::default()),
        };
        let snapshot = service.read().unwrap();
        let report = service
            .apply(&input(&snapshot, GitProxyMode::Disabled, None))
            .unwrap();
        assert_eq!(report.status, ApplyStatus::Partial);
        assert!(report.snapshot.is_none());
        assert!(sandbox.contents().contains("proxy ="));
        assert!(service.state.lock().unwrap().snapshot.is_none());
    }

    #[test]
    fn malformed_config_does_not_expose_raw_diagnostics() {
        let sandbox = Sandbox::new();
        sandbox.write("[http secret-password\n");
        let error = sandbox.service().read().unwrap_err();
        assert_eq!(error.code, ProxyErrorCode::ReadFailed);
        assert!(!error.message.contains("secret-password"));
        assert!(!error.message.contains("global.gitconfig"));
    }

    struct MissingGit;

    struct PermissionDeniedRunner(IsolatedRunner);

    impl GitRunner for PermissionDeniedRunner {
        fn run(&self, args: &[&str]) -> io::Result<Output> {
            if args.contains(&"--replace-all") || args.contains(&"--unset-all") {
                #[cfg(windows)]
                let status = {
                    use std::os::windows::process::ExitStatusExt;
                    std::process::ExitStatus::from_raw(1)
                };
                #[cfg(unix)]
                let status = {
                    use std::os::unix::process::ExitStatusExt;
                    std::process::ExitStatus::from_raw(1 << 8)
                };
                return Ok(Output {
                    status,
                    stdout: Vec::new(),
                    stderr: b"permission denied: http://alice:private-password@localhost:7890"
                        .to_vec(),
                });
            }
            self.0.run(args)
        }
    }

    #[test]
    fn permission_failure_preserves_file_and_hides_diagnostics() {
        let sandbox = Sandbox::new();
        sandbox.write("[http]\n proxy = http://old:7890\n");
        let original = sandbox.contents();
        let service = GitProxyService {
            runner: Box::new(PermissionDeniedRunner(IsolatedRunner {
                root: sandbox.root.clone(),
                fail_readback: false,
                pin_global: true,
                wrote: AtomicBool::new(false),
            })),
            state: Mutex::new(ReadState::default()),
        };
        let snapshot = service.read().unwrap();
        let report = service
            .apply(&input(&snapshot, GitProxyMode::Disabled, None))
            .unwrap();
        assert_eq!(report.status, ApplyStatus::Failed);
        assert!(report.message.contains("权限"));
        assert!(!report.message.contains("alice"));
        assert!(!report.message.contains("private-password"));
        assert_eq!(sandbox.contents(), original);
    }

    impl GitRunner for MissingGit {
        fn run(&self, _args: &[&str]) -> io::Result<Output> {
            Err(io::ErrorKind::NotFound.into())
        }
    }

    #[test]
    fn missing_git_is_visible_and_cannot_write() {
        let service = GitProxyService {
            runner: Box::new(MissingGit),
            state: Mutex::new(ReadState::default()),
        };
        let snapshot = service.read().unwrap();
        assert!(!snapshot.available);
        assert!(!snapshot.writable);
        assert!(!snapshot.warnings.is_empty());
        assert_eq!(
            service
                .apply(&input(&snapshot, GitProxyMode::Disabled, None))
                .unwrap()
                .status,
            ApplyStatus::Failed
        );
    }

    #[test]
    fn invalid_custom_value_leaves_file_unchanged() {
        let sandbox = Sandbox::new();
        let service = sandbox.service();
        let snapshot = service.read().unwrap();
        let error = service
            .apply(&input(
                &snapshot,
                GitProxyMode::Custom,
                Some("http://alice:secret@localhost:7890/path"),
            ))
            .unwrap_err();
        assert_eq!(error.code, ProxyErrorCode::ValidationFailed);
        assert!(!error.message.contains("secret"));
        assert!(!sandbox.config().exists());
    }

    #[test]
    fn xdg_only_proxy_can_be_updated_in_its_own_source() {
        let sandbox = Sandbox::new();
        fs::create_dir_all(sandbox.root.join("git")).unwrap();
        fs::write(
            sandbox.root.join("git/config"),
            "[http]\n proxy = http://xdg:7890\n",
        )
        .unwrap();
        let service = GitProxyService {
            runner: Box::new(IsolatedRunner {
                root: sandbox.root.clone(),
                fail_readback: false,
                pin_global: false,
                wrote: AtomicBool::new(false),
            }),
            state: Mutex::new(ReadState::default()),
        };
        let snapshot = service.read().unwrap();
        assert_eq!(snapshot.mode, GitProxyMode::Custom);
        assert!(snapshot.writable);
        let report = service
            .apply(&input(&snapshot, GitProxyMode::Disabled, None))
            .unwrap();
        assert_eq!(report.status, ApplyStatus::Applied);
        assert!(!sandbox.root.join(".gitconfig").exists());
        assert!(
            !fs::read_to_string(sandbox.root.join("git/config"))
                .unwrap()
                .contains("http://xdg:7890")
        );
    }

    #[test]
    fn xdg_proxy_with_empty_user_config_is_read_only() {
        let sandbox = Sandbox::new();
        fs::create_dir_all(sandbox.root.join("git")).unwrap();
        fs::write(
            sandbox.root.join("git/config"),
            "[http]\n proxy = http://xdg:7890\n",
        )
        .unwrap();
        fs::write(sandbox.root.join(".gitconfig"), "").unwrap();
        let service = GitProxyService {
            runner: Box::new(IsolatedRunner {
                root: sandbox.root.clone(),
                fail_readback: false,
                pin_global: false,
                wrote: AtomicBool::new(false),
            }),
            state: Mutex::new(ReadState::default()),
        };
        let snapshot = service.read().unwrap();
        assert_eq!(snapshot.mode, GitProxyMode::Custom);
        assert!(!snapshot.writable);
        assert!(
            snapshot
                .warnings
                .iter()
                .any(|warning| warning.contains("写入目标不同"))
        );
        let report = service
            .apply(&input(&snapshot, GitProxyMode::Disabled, None))
            .unwrap();
        assert_eq!(report.status, ApplyStatus::Failed);
        assert_eq!(
            fs::read_to_string(sandbox.root.join(".gitconfig")).unwrap(),
            ""
        );
        assert!(
            fs::read_to_string(sandbox.root.join("git/config"))
                .unwrap()
                .contains("http://xdg:7890")
        );
    }

    #[test]
    fn multiple_root_sources_are_read_only() {
        let raw = RawSnapshot {
            available: true,
            direct: vec![("file:first".to_owned(), "http://proxy:7890".to_owned())],
            effective: vec![("file:first".to_owned(), "http://proxy:7890".to_owned())],
            keys: vec![
                ("file:first".to_owned(), "http.proxy".to_owned()),
                ("file:second".to_owned(), "user.name".to_owned()),
            ],
            write_target: "first".to_owned(),
        };
        assert!(!raw.writable());
        assert!(
            raw.warnings()
                .iter()
                .any(|warning| warning.contains("多个来源"))
        );
    }
}
