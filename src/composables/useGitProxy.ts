import { computed, reactive, ref } from "vue";
import { applyGitProxySettings, getGitProxySnapshot } from "../api/proxy";
import type { GitApplyReport, GitProxyMode, GitProxySnapshot } from "../generated/bindings";
import { maskSensitiveText, readableError } from "../utils/sensitive";

/** Git 单独保存；脱敏地址从不作为原始凭据写回，proxy=null 交由后端保留原值。 */
export function useGitProxy() {
  const snapshot = ref<GitProxySnapshot | null>(null);
  const draft = reactive<{ mode: GitProxyMode; proxy: string }>({ mode: "inherit", proxy: "" });
  const state = reactive({
    loading: false,
    saving: false,
    loadError: "",
    saveError: "",
    lastReadAt: "",
    submitCount: 0,
  });
  const report = ref<GitApplyReport | null>(null);
  const modeDirty = ref(false);
  const proxyDirty = ref(false);
  const dirty = computed(() => modeDirty.value || proxyDirty.value);
  const error = computed(() =>
    draft.mode === "custom" && proxyDirty.value && !draft.proxy.trim()
      ? "请输入完整的代理地址。"
      : "",
  );
  function sync(next: GitProxySnapshot) {
    snapshot.value = next;
    draft.mode = next.mode;
    draft.proxy = maskSensitiveText(next.proxy ?? "");
    modeDirty.value = false;
    proxyDirty.value = false;
    state.lastReadAt = new Date().toLocaleTimeString("zh-CN", { hour12: false });
  }
  async function reload() {
    if (state.loading || state.saving) {
      return;
    }
    if (dirty.value && !window.confirm("重新读取将丢弃 Git 页未保存的修改，是否继续？")) {
      return;
    }
    state.loading = true;
    state.loadError = "";
    try {
      sync(await getGitProxySnapshot());
      state.saveError = "";
      state.submitCount = 0;
      report.value = null;
    } catch (error) {
      state.loadError = `读取失败：${readableError(error)}。请检查 Git 安装和配置后重新读取。`;
    } finally {
      state.loading = false;
    }
  }
  async function save() {
    if (!snapshot.value?.writable || state.loading || state.saving) {
      return;
    }
    state.submitCount += 1;
    state.saveError = "";
    if (
      error.value ||
      (draft.mode === "custom" && !proxyDirty.value && snapshot.value.mode !== "custom")
    ) {
      state.saveError = error.value || "请输入完整的代理地址。";
      return;
    }
    state.saving = true;
    report.value = null;
    try {
      const next = await applyGitProxySettings({
        mode: draft.mode,
        proxy: proxyDirty.value ? draft.proxy : null,
        expectedRevision: snapshot.value.revision,
      });
      report.value = next;
      if ((next.status === "applied" || next.status === "unchanged") && next.snapshot) {
        sync(next.snapshot);
        state.submitCount = 0;
      }
      // 失败或部分写入时保留草稿和原 revision，让后端阻止基于过期快照的覆盖。
    } catch (error) {
      state.saveError = `保存未完成：${readableError(error)}。请重新读取核对；草稿已保留。`;
    } finally {
      state.saving = false;
    }
  }
  function setMode(mode: GitProxyMode) {
    draft.mode = mode;
    modeDirty.value = true;
  }
  function setProxy(proxy: string) {
    draft.proxy = proxy;
    proxyDirty.value = true;
  }
  return { snapshot, draft, state, report, dirty, error, reload, save, setMode, setProxy };
}
