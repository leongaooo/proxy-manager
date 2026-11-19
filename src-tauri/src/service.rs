use std::sync::Mutex;

use crate::contracts::{
    ApplyReport, ApplyRequest, ApplyStatus, ProxyError, ProxySnapshot, SectionResult,
    SystemProxyInput, TerminalProxyInput,
};
use crate::platform::{PlatformAdapter, combine_status};
use crate::rules::{normalize_system, normalize_terminal};

/// 应用入口按业务区块编排保存；平台副作用不会隐藏在 UI 或纯规则中。
pub struct ProxyService {
    adapter: Box<dyn PlatformAdapter>,
    save_lock: Mutex<()>,
}

impl ProxyService {
    pub fn new(adapter: Box<dyn PlatformAdapter>) -> Self {
        Self {
            adapter,
            save_lock: Mutex::new(()),
        }
    }

    pub fn read(&self, target: Option<&str>) -> Result<ProxySnapshot, ProxyError> {
        self.adapter.read_snapshot(target)
    }

    pub fn apply(&self, request: &ApplyRequest) -> Result<ApplyReport, ProxyError> {
        let _guard = self
            .save_lock
            .lock()
            .map_err(|_| ProxyError::persist("上一次保存异常中断，请重新读取实际配置后再试"))?;
        let target = request
            .system
            .as_ref()
            .and_then(|input| input.target.as_deref());
        let initial_snapshot = self.adapter.read_snapshot(target)?;

        let terminal = request.terminal.as_ref().map(normalize_terminal);
        let system = request.system.as_ref().map(normalize_system);
        if terminal.as_ref().is_some_and(Result::is_err)
            || system.as_ref().is_some_and(Result::is_err)
        {
            return Ok(self.validation_failed_report(request, initial_snapshot, terminal, system));
        }

        let terminal_input = terminal.transpose()?.filter(|_| request.terminal.is_some());
        let system_input = system.transpose()?.filter(|_| request.system.is_some());
        let terminal_result = terminal_input
            .as_ref()
            .map(|input| self.apply_terminal_section(input));
        let system_result = system_input
            .as_ref()
            .map(|input| self.apply_system_section(input));

        let status = combine_status(terminal_result.as_ref(), system_result.as_ref());
        let snapshot = self.adapter.read_snapshot(target)?;
        Ok(ApplyReport {
            status,
            terminal: terminal_result,
            system: system_result,
            snapshot,
        })
    }

    fn apply_terminal_section(&self, input: &TerminalProxyInput) -> SectionResult {
        self.adapter
            .apply_terminal(input)
            .unwrap_or_else(SectionResult::failed)
    }

    fn apply_system_section(&self, input: &SystemProxyInput) -> SectionResult {
        self.adapter
            .apply_system(input)
            .unwrap_or_else(SectionResult::failed)
    }

    fn validation_failed_report(
        &self,
        request: &ApplyRequest,
        snapshot: ProxySnapshot,
        terminal: Option<Result<TerminalProxyInput, ProxyError>>,
        system: Option<Result<SystemProxyInput, ProxyError>>,
    ) -> ApplyReport {
        let other_section_message =
            "输入有效，但另一区块校验失败；本次未写入任何配置，请修正后重新保存";
        let terminal_result = request.terminal.as_ref().map(|_| match terminal {
            Some(Err(error)) => SectionResult::failed(error),
            _ => SectionResult::failed(ProxyError::validation(other_section_message)),
        });
        let system_result = request.system.as_ref().map(|_| match system {
            Some(Err(error)) => SectionResult::failed(error),
            _ => SectionResult::failed(ProxyError::validation(other_section_message)),
        });
        ApplyReport {
            status: ApplyStatus::Failed,
            terminal: terminal_result,
            system: system_result,
            snapshot,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::contracts::{
        Platform, SectionStatus, SystemSnapshot, TerminalSnapshot, TerminalValues,
    };

    #[derive(Default)]
    struct FakeAdapter {
        writes: Mutex<Vec<&'static str>>,
        fail_terminal: bool,
        fail_system: bool,
        target: Mutex<Option<String>>,
    }

    fn snapshot() -> ProxySnapshot {
        ProxySnapshot {
            platform: Platform::Windows,
            terminal: TerminalSnapshot {
                process: TerminalValues::default(),
                persisted: TerminalValues::default(),
                persistence_label: "fake".to_owned(),
                launchd: None,
                warnings: Vec::new(),
            },
            system: SystemSnapshot {
                enabled: false,
                http: None,
                https: None,
                socks: None,
                bypass: Vec::new(),
                available_targets: Vec::new(),
                selected_target: None,
                auto_config: None,
                warnings: Vec::new(),
            },
            warnings: Vec::new(),
        }
    }

    impl PlatformAdapter for Arc<FakeAdapter> {
        fn read_snapshot(&self, target: Option<&str>) -> Result<ProxySnapshot, ProxyError> {
            *self.target.lock().unwrap() = target.map(ToOwned::to_owned);
            Ok(snapshot())
        }

        fn apply_terminal(&self, _input: &TerminalProxyInput) -> Result<SectionResult, ProxyError> {
            self.writes.lock().unwrap().push("terminal");
            if self.fail_terminal {
                Err(ProxyError::persist("终端 fake 写入失败，请重新读取"))
            } else {
                Ok(SectionResult::applied("终端 fake 已写入并回读"))
            }
        }

        fn apply_system(&self, _input: &SystemProxyInput) -> Result<SectionResult, ProxyError> {
            self.writes.lock().unwrap().push("system");
            if self.fail_system {
                Err(ProxyError::persist("系统 fake 写入失败，请重新读取"))
            } else {
                Ok(SectionResult::applied("系统 fake 已写入并回读"))
            }
        }
    }

    #[test]
    fn apply_should_not_write_when_any_input_is_invalid() {
        let adapter = Arc::new(FakeAdapter::default());
        let service = ProxyService::new(Box::new(adapter.clone()));
        let report = service
            .apply(&ApplyRequest {
                terminal: Some(TerminalProxyInput {
                    http_proxy: Some("http://user:secret@localhost:7890".to_owned()),
                    ..TerminalProxyInput::default()
                }),
                system: Some(SystemProxyInput {
                    enabled: true,
                    http: None,
                    https: None,
                    socks: None,
                    bypass: Vec::new(),
                    target: Some(" Wi-Fi ".to_owned()),
                }),
            })
            .unwrap();
        assert_eq!(report.status, ApplyStatus::Failed);
        assert_eq!(
            report.terminal.unwrap().code,
            Some(crate::contracts::ProxyErrorCode::ValidationFailed)
        );
        assert_eq!(
            report.system.unwrap().code,
            Some(crate::contracts::ProxyErrorCode::ValidationFailed)
        );
        assert_eq!(adapter.writes.lock().unwrap().len(), 0);
    }

    #[test]
    fn apply_should_report_partial_result_and_keep_other_section_success() {
        let adapter = Arc::new(FakeAdapter {
            fail_system: true,
            ..FakeAdapter::default()
        });
        let service = ProxyService::new(Box::new(adapter.clone()));
        let report = service
            .apply(&ApplyRequest {
                terminal: Some(TerminalProxyInput::default()),
                system: Some(SystemProxyInput {
                    enabled: false,
                    http: None,
                    https: None,
                    socks: None,
                    bypass: Vec::new(),
                    target: Some("Wi-Fi".to_owned()),
                }),
            })
            .unwrap();
        assert_eq!(report.status, ApplyStatus::Partial);
        assert_eq!(report.terminal.unwrap().status, SectionStatus::Applied);
        assert_eq!(report.system.unwrap().status, SectionStatus::Failed);
        assert_eq!(*adapter.target.lock().unwrap(), Some("Wi-Fi".to_owned()));
    }

    #[test]
    fn apply_should_report_failed_when_both_sections_fail() {
        let adapter = Arc::new(FakeAdapter {
            fail_terminal: true,
            fail_system: true,
            ..FakeAdapter::default()
        });
        let service = ProxyService::new(Box::new(adapter.clone()));
        let report = service
            .apply(&ApplyRequest {
                terminal: Some(TerminalProxyInput::default()),
                system: Some(SystemProxyInput {
                    enabled: false,
                    http: None,
                    https: None,
                    socks: None,
                    bypass: Vec::new(),
                    target: None,
                }),
            })
            .unwrap();
        assert_eq!(report.status, ApplyStatus::Failed);
        assert_eq!(adapter.writes.lock().unwrap().len(), 2);
    }
}
