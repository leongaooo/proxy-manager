import { computed, reactive, ref } from "vue";
import { applyProxySettings, getProxySnapshot } from "../api/proxy";
import type {
  ApplyReport,
  ApplyRequest,
  ProxyEndpoint,
  ProxySnapshot,
  SystemProxyInput,
  TerminalProxyInput,
  TerminalValues,
} from "../generated/bindings";
import { maskSensitiveText, readableError } from "../utils/sensitive";

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

const terminalFields: TerminalField[] = ["httpProxy", "httpsProxy", "allProxy", "noProxy"];

function createTerminalDraft(): TerminalDraft {
  return {
    httpProxy: "",
    httpsProxy: "",
    allProxy: "",
    noProxy: "",
  };
}

function createEndpointDraft(): EndpointDraft {
  return { host: "", port: "" };
}

function createSystemDraft(): SystemDraft {
  return {
    enabled: false,
    http: createEndpointDraft(),
    https: createEndpointDraft(),
    socks: createEndpointDraft(),
    bypassText: "",
    target: "",
  };
}

function endpointDraft(endpoint: ProxyEndpoint | null): EndpointDraft {
  return {
    host: endpoint?.host ?? "",
    port: endpoint === null ? "" : String(endpoint.port),
  };
}

export function normalizeBypass(value: string): string[] {
  return value
    .split(/[\n,，]/)
    .map((entry) => entry.trim())
    .filter((entry) => entry.length > 0);
}

function toEndpointInput(draft: EndpointDraft): ProxyEndpoint | null {
  const host = draft.host.trim();
  if (!host && !draft.port.trim()) {
    return null;
  }
  return { host, port: Number(draft.port) };
}

/**
 * 权威快照只来自后端读取或保存回读；表单草稿始终单独保存。
 * 脱敏的终端字段只有在用户实际编辑后才会进入请求，其他字段以 null 表示保持原值。
 */
export function useProxySettings() {
  const snapshot = ref<ProxySnapshot | null>(null);
  const report = ref<ApplyReport | null>(null);
  const loading = ref(false);
  const saving = ref(false);
  const loadError = ref("");
  const saveError = ref("");
  const submitCount = ref(0);
  const lastReadAt = ref("");
  const systemDirty = ref(false);

  const terminalDraft = reactive<TerminalDraft>(createTerminalDraft());
  const systemDraft = reactive<SystemDraft>(createSystemDraft());
  const terminalDirty = reactive<Record<TerminalField, boolean>>({
    httpProxy: false,
    httpsProxy: false,
    allProxy: false,
    noProxy: false,
  });

  function syncTerminalDraft(values: TerminalValues) {
    for (const field of terminalFields) {
      terminalDraft[field] = maskSensitiveText(values[field] ?? "");
      terminalDirty[field] = false;
    }
  }

  function syncSystemDraft(nextSnapshot: ProxySnapshot) {
    const system = nextSnapshot.system;
    systemDraft.enabled = system.enabled;
    Object.assign(systemDraft.http, endpointDraft(system.http));
    Object.assign(systemDraft.https, endpointDraft(system.https));
    Object.assign(systemDraft.socks, endpointDraft(system.socks));
    systemDraft.bypassText = system.bypass.join("\n");
    systemDraft.target = system.selectedTarget ?? system.availableTargets[0] ?? "";
    systemDirty.value = false;
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
      const endpoint = systemDraft[protocol];
      const host = endpoint.host.trim();
      const port = endpoint.port.trim();
      if (!host && port) {
        errors[`${protocol}.host`] = "已填写端口，请同时填写对应主机。";
      }
      if (host && !port) {
        errors[`${protocol}.port`] = "已填写主机，请同时填写对应端口。";
      } else if (port && (!/^\d+$/.test(port) || Number(port) < 1 || Number(port) > 65535)) {
        errors[`${protocol}.port`] = "端口必须是 1 到 65535 之间的整数。";
      }
    }
    if (
      snapshot.value?.platform === "macos" &&
      snapshot.value.system.availableTargets.length > 0 &&
      !systemDraft.target.trim()
    ) {
      errors.target = "请选择要读取和保存的 macOS 网络服务。";
    }
    return errors;
  });

  const hasSystemErrors = computed(() => Object.keys(systemErrors.value).length > 0);

  async function readSnapshot(target?: string, scope: "all" | "system" = "all") {
    if (loading.value || saving.value) {
      return;
    }
    loading.value = true;
    loadError.value = "";
    saveError.value = "";
    try {
      const nextSnapshot = await getProxySnapshot(target);
      snapshot.value = nextSnapshot;
      lastReadAt.value = new Date().toLocaleString("zh-CN", { hour12: false });
      if (scope === "all") {
        syncTerminalDraft(nextSnapshot.terminal.persisted);
      }
      syncSystemDraft(nextSnapshot);
      report.value = null;
      submitCount.value = 0;
    } catch (error) {
      loadError.value = `读取代理设置失败：${readableError(error)}。请检查系统权限或网络服务选择后重试。`;
    } finally {
      loading.value = false;
    }
  }

  async function reload() {
    const target = snapshot.value?.system.selectedTarget ?? undefined;
    await readSnapshot(target);
  }

  async function selectTarget(target: string) {
    if (!target || target === systemDraft.target) {
      return;
    }
    await readSnapshot(target, "system");
  }

  function createRequest(): ApplyRequest {
    const terminal: TerminalProxyInput = {
      httpProxy: terminalDirty.httpProxy ? terminalDraft.httpProxy : null,
      httpsProxy: terminalDirty.httpsProxy ? terminalDraft.httpsProxy : null,
      allProxy: terminalDirty.allProxy ? terminalDraft.allProxy : null,
      noProxy: terminalDirty.noProxy ? terminalDraft.noProxy : null,
    };
    const system: SystemProxyInput = {
      enabled: systemDraft.enabled,
      http: toEndpointInput(systemDraft.http),
      https: toEndpointInput(systemDraft.https),
      socks: toEndpointInput(systemDraft.socks),
      bypass: normalizeBypass(systemDraft.bypassText),
      target: systemDraft.target || null,
    };
    return { terminal, system };
  }

  async function saveAll(): Promise<ApplyReport | null> {
    if (!snapshot.value || loading.value || saving.value) {
      return null;
    }
    submitCount.value += 1;
    saveError.value = "";
    if (hasSystemErrors.value) {
      saveError.value = "尚未保存：请先修正系统代理中标记的主机、端口或网络服务，再重新提交。";
      return null;
    }

    saving.value = true;
    report.value = null;
    try {
      const nextReport = await applyProxySettings(createRequest());
      report.value = nextReport;
      snapshot.value = nextReport.snapshot;
      lastReadAt.value = new Date().toLocaleString("zh-CN", { hour12: false });
      if (nextReport.terminal?.status !== "failed") {
        syncTerminalDraft(nextReport.snapshot.terminal.persisted);
      }
      if (nextReport.system?.status !== "failed") {
        syncSystemDraft(nextReport.snapshot);
      }
      if (nextReport.terminal?.status !== "failed" && nextReport.system?.status !== "failed") {
        submitCount.value = 0;
      }
      return nextReport;
    } catch (error) {
      saveError.value = `保存未完成：${readableError(error)}。请按原因修正后重试；若结果不明，请先重新读取核对，避免重复写入。`;
      return null;
    } finally {
      saving.value = false;
    }
  }

  return {
    snapshot,
    report,
    loading,
    saving,
    loadError,
    saveError,
    submitCount,
    lastReadAt,
    systemDirty,
    terminalDraft,
    systemDraft,
    terminalDirty,
    systemErrors,
    reload,
    selectTarget,
    saveAll,
    markTerminalDirty,
    markSystemDirty,
  };
}
