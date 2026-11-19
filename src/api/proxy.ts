import { invoke } from "@tauri-apps/api/core";
import type {
  ApplyReport,
  ApplyRequest,
  ProxySnapshot,
  GitApplyReport,
  GitProxyInput,
  GitProxySnapshot,
} from "../generated/bindings";

export function getProxySnapshot(target?: string): Promise<ProxySnapshot> {
  return invoke("get_proxy_snapshot", target === undefined ? {} : { target });
}
export function applyProxySettings(request: ApplyRequest): Promise<ApplyReport> {
  return invoke("apply_proxy_settings", { request });
}
export function getGitProxySnapshot(): Promise<GitProxySnapshot> {
  return invoke("get_git_proxy_snapshot");
}
export function applyGitProxySettings(request: GitProxyInput): Promise<GitApplyReport> {
  return invoke("apply_git_proxy_settings", { request });
}
