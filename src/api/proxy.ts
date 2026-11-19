import { invoke } from "@tauri-apps/api/core";
import type { ApplyReport, ApplyRequest, ProxySnapshot } from "../generated/bindings";

export function getProxySnapshot(target?: string): Promise<ProxySnapshot> {
  return invoke("get_proxy_snapshot", target === undefined ? {} : { target });
}

export function applyProxySettings(request: ApplyRequest): Promise<ApplyReport> {
  return invoke("apply_proxy_settings", { request });
}
