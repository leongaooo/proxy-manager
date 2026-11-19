use tauri::State;

use crate::contracts::{ApplyReport, ApplyRequest, ProxyError, ProxySnapshot};
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
