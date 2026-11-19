import { computed, reactive, ref } from "vue";
import { applyProxySettings, getProxySnapshot } from "../api/proxy";
import type {
  ApplyReport,
  ApplyRequest,
  ProxyEndpoint,
  ProxySnapshot,
  SystemSnapshot,
  SystemProxyInput,
  TerminalProxyInput,
  TerminalValues,
} from "../generated/bindings";
import { maskSensitiveText, readableError } from "../utils/sensitive";

export type ProxyPage = "terminal" | "system";
export type TerminalField = keyof TerminalValues;
export type SystemProtocol = "http" | "https" | "socks";
export type SystemField = `${SystemProtocol}.host` | `${SystemProtocol}.port` | "target";
export interface EndpointDraft {
  host: string;
  port: string;
}
export interface TerminalDraft {
  httpProxy: string;
  httpsProxy: string;
  allProxy: string;
  noProxy: string;
}
export interface SystemDraft {
  enabled: boolean;
  http: EndpointDraft;
  https: EndpointDraft;
  socks: EndpointDraft;
  bypassText: string;
  target: string;
}
export interface PageState {
  loading: boolean;
  saving: boolean;
  loadError: string;
  saveError: string;
  submitCount: number;
  lastReadAt: string;
  report: ApplyReport | null;
}
export function createPageState(): PageState {
  return {
    loading: false,
    saving: false,
    loadError: "",
    saveError: "",
    submitCount: 0,
    lastReadAt: "",
    report: null,
  };
}
const terminalFields: TerminalField[] = ["httpProxy", "httpsProxy", "allProxy", "noProxy"];
function endpointDraft(endpoint: ProxyEndpoint | null): EndpointDraft {
  return { host: endpoint?.host ?? "", port: endpoint === null ? "" : String(endpoint.port) };
}
export function normalizeBypass(value: string): string[] {
  return value
    .split(/[\n,，]/)
    .map((entry) => entry.trim())
    .filter(Boolean);
}
function toEndpointInput(draft: EndpointDraft): ProxyEndpoint | null {
  const host = draft.host.trim();
  return !host && !draft.port.trim() ? null : { host, port: Number(draft.port) };
}

/** 每页拥有独立读写状态和草稿；完整后端快照仅合并本次操作的区块，避免跨页覆盖。 */
export function useProxySettings() {
  const snapshot = ref<ProxySnapshot | null>(null);
  const pages = reactive({ terminal: createPageState(), system: createPageState() });
  const terminalDraft = reactive<TerminalDraft>({
    httpProxy: "",
    httpsProxy: "",
    allProxy: "",
    noProxy: "",
  });
  const systemDraft = reactive<SystemDraft>({
    enabled: false,
    http: endpointDraft(null),
    https: endpointDraft(null),
    socks: endpointDraft(null),
    bypassText: "",
    target: "",
  });
  const terminalDirty = reactive<Record<TerminalField, boolean>>({
    httpProxy: false,
    httpsProxy: false,
    allProxy: false,
    noProxy: false,
  });
  const systemDirty = ref(false);
  const dirty = computed(() => ({
    terminal: terminalFields.some((field) => terminalDirty[field]),
    system: systemDirty.value,
  }));

  function syncTerminalDraft(values: TerminalValues) {
    for (const field of terminalFields) {
      terminalDraft[field] = maskSensitiveText(values[field] ?? "");
      terminalDirty[field] = false;
    }
  }
  function syncSystemDraft(system: SystemSnapshot) {
    systemDraft.enabled = system.enabled;
    Object.assign(systemDraft.http, endpointDraft(system.http));
    Object.assign(systemDraft.https, endpointDraft(system.https));
    Object.assign(systemDraft.socks, endpointDraft(system.socks));
    systemDraft.bypassText = system.bypass.join("\n");
    systemDraft.target = system.selectedTarget ?? system.availableTargets[0] ?? "";
    systemDirty.value = false;
  }
  function mergeSnapshot(next: ProxySnapshot, scope: ProxyPage) {
    snapshot.value = snapshot.value
      ? { ...snapshot.value, platform: next.platform, [scope]: next[scope] }
      : next;
  }
  function markTerminalDirty(field: TerminalField) {
    terminalDirty[field] = true;
  }
  function markSystemDirty() {
    systemDirty.value = true;
  }

  const systemErrors = computed(() => {
    const errors: Partial<Record<SystemField, string>> = {};
    const protocols: SystemProtocol[] = ["http", "https", "socks"];
    for (const protocol of systemDraft.enabled ? protocols : []) {
      const { host: rawHost, port: rawPort } = systemDraft[protocol];
      const host = rawHost.trim();
      const port = rawPort.trim();
      if (!host && port) {
        errors[`${protocol}.host`] = "请填写主机。";
      }
      if (host && !port) {
        errors[`${protocol}.port`] = "请填写端口。";
      } else if (port && (!/^\d+$/.test(port) || Number(port) < 1 || Number(port) > 65535)) {
        errors[`${protocol}.port`] = "端口须为 1 到 65535 的整数。";
      }
    }
    if (snapshot.value?.platform === "macos" && !systemDraft.target.trim()) {
      errors.target = "请选择网络服务。";
    }
    return errors;
  });

  async function readPage(scope: ProxyPage, target?: string) {
    const state = pages[scope];
    if (state.loading || state.saving) {
      return;
    }
    state.loading = true;
    state.loadError = "";
    try {
      const next = await getProxySnapshot(target);
      // 首次读取建立两页基线；之后仅允许当前操作替换自己的草稿。
      const initial = snapshot.value === null;
      mergeSnapshot(next, scope);
      if (scope === "terminal" || initial) {
        syncTerminalDraft(next.terminal.persisted);
      }
      if (scope === "system" || initial) {
        syncSystemDraft(next.system);
      }
      state.lastReadAt = new Date().toLocaleTimeString("zh-CN", { hour12: false });
      state.report = null;
      state.saveError = "";
      state.submitCount = 0;
    } catch (error) {
      state.loadError = `读取失败：${readableError(error)}。请检查权限后重新读取；草稿已保留。`;
    } finally {
      state.loading = false;
    }
  }
  async function reload(scope: ProxyPage = "terminal") {
    if (pages[scope].loading || pages[scope].saving) {
      return;
    }
    if (dirty.value[scope] && !window.confirm("重新读取将丢弃当前页未保存的修改，是否继续？")) {
      return;
    }
    await readPage(scope, snapshot.value?.system.selectedTarget ?? undefined);
  }
  async function selectTarget(target: string) {
    if (!target || target === systemDraft.target || pages.system.loading || pages.system.saving) {
      return;
    }
    if (dirty.value.system && !window.confirm("切换网络服务将丢弃系统页未保存的修改，是否继续？")) {
      return;
    }
    await readPage("system", target);
  }
  function createRequest(scope: ProxyPage): ApplyRequest {
    if (scope === "terminal") {
      const terminal: TerminalProxyInput = {
        httpProxy: terminalDirty.httpProxy ? terminalDraft.httpProxy : null,
        httpsProxy: terminalDirty.httpsProxy ? terminalDraft.httpsProxy : null,
        allProxy: terminalDirty.allProxy ? terminalDraft.allProxy : null,
        noProxy: terminalDirty.noProxy ? terminalDraft.noProxy : null,
      };
      return { terminal, system: null };
    }
    const system: SystemProxyInput = {
      enabled: systemDraft.enabled,
      http: toEndpointInput(systemDraft.http),
      https: toEndpointInput(systemDraft.https),
      socks: toEndpointInput(systemDraft.socks),
      bypass: normalizeBypass(systemDraft.bypassText),
      target: systemDraft.target || null,
    };
    return { terminal: null, system };
  }
  async function save(scope: ProxyPage): Promise<ApplyReport | null> {
    const state = pages[scope];
    if (!snapshot.value || state.loading || state.saving) {
      return null;
    }
    state.submitCount += 1;
    state.saveError = "";
    if (scope === "system" && Object.keys(systemErrors.value).length) {
      state.saveError = "请修正标记的主机、端口或网络服务。";
      return null;
    }
    state.saving = true;
    state.report = null;
    try {
      const next = await applyProxySettings(createRequest(scope));
      state.report = next;
      mergeSnapshot(next.snapshot, scope);
      const result = next[scope];
      // 缺席区块与失败均不能清草稿；只有明确成功且完成回读才同步。
      if (result && (result.status === "applied" || result.status === "unchanged")) {
        if (scope === "terminal") {
          syncTerminalDraft(next.snapshot.terminal.persisted);
        } else {
          syncSystemDraft(next.snapshot.system);
        }
        state.submitCount = 0;
        state.lastReadAt = new Date().toLocaleTimeString("zh-CN", { hour12: false });
      } else if (!result) {
        state.saveError = "未收到当前页保存结果，请重新读取核对；草稿已保留。";
      }
      return next;
    } catch (error) {
      state.saveError = `保存未完成：${readableError(error)}。请重新读取核对后重试；草稿已保留。`;
      return null;
    } finally {
      state.saving = false;
    }
  }
  return {
    snapshot,
    pages,
    dirty,
    terminalDraft,
    systemDraft,
    terminalDirty,
    systemDirty,
    systemErrors,
    reload,
    selectTarget,
    save,
    markTerminalDirty,
    markSystemDirty,
  };
}
