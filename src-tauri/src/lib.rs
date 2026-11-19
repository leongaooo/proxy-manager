mod commands;
pub mod contracts;
pub mod git_proxy;
pub mod platform;
pub mod rules;
pub mod service;

use service::ProxyService;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(ProxyService::new(platform::current_adapter()))
        .manage(git_proxy::GitProxyService::default())
        .invoke_handler(tauri::generate_handler![
            commands::get_proxy_snapshot,
            commands::apply_proxy_settings,
            commands::get_git_proxy_snapshot,
            commands::apply_git_proxy_settings
        ])
        .run(tauri::generate_context!())
        .expect("代理设置应用启动失败");
}
