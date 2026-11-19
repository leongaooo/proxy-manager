use tauri::State;

use crate::contracts::{ApplyReport, ApplyRequest, ProxyError, ProxySnapshot};
use crate::contracts::{GitApplyReport, GitProxyInput, GitProxySnapshot};
use crate::git_proxy::GitProxyService;
use crate::service::ProxyService;

#[tauri::command]
pub fn get_proxy_snapshot(
    target: Option<String>,
    service: State<'_, ProxyService>,
) -> Result<ProxySnapshot, ProxyError> {
    service.read(target.as_deref())
}

#[tauri::command]
pub fn apply_proxy_settings(
    request: ApplyRequest,
    service: State<'_, ProxyService>,
) -> Result<ApplyReport, ProxyError> {
    service.apply(&request)
}

#[tauri::command]
pub async fn get_git_proxy_snapshot(
    service: State<'_, GitProxyService>,
) -> Result<GitProxySnapshot, ProxyError> {
    service.read()
}

#[tauri::command]
pub async fn apply_git_proxy_settings(
    request: GitProxyInput,
    service: State<'_, GitProxyService>,
) -> Result<GitApplyReport, ProxyError> {
    service.apply(&request)
}
