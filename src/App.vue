<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import ApplyReport from "./components/ApplyReport.vue";
import GitProxyForm from "./components/GitProxyForm.vue";
import SystemProxyForm from "./components/SystemProxyForm.vue";
import TerminalProxyForm from "./components/TerminalProxyForm.vue";
import {
  useProxySettings,
  type ProxyPage,
  type TerminalField,
} from "./composables/useProxySettings";
import { useGitProxy } from "./composables/useGitProxy";
import { maskSensitiveText } from "./utils/sensitive";

type Tab = ProxyPage | "git";
const tabs: { key: Tab; label: string; detail: string }[] = [
  { key: "terminal", label: "终端代理", detail: "环境变量" },
  { key: "system", label: "系统代理", detail: "操作系统" },
  { key: "git", label: "Git 代理", detail: "全局配置" },
];
const active = ref<Tab>("terminal");
const {
  snapshot,
  pages,
  dirty,
  terminalDraft,
  systemDraft,
  systemErrors,
  reload,
  selectTarget,
  save,
  markTerminalDirty,
  markSystemDirty,
} = useProxySettings();
const git = useGitProxy();
const current = computed(() => (active.value === "git" ? git.state : pages[active.value]));
const isDirty = computed(() =>
  active.value === "git" ? git.dirty.value : dirty.value[active.value],
);
const canSave = computed(() =>
  active.value === "git"
    ? !!git.snapshot.value?.writable
    : !!snapshot.value && !(active.value === "system" && snapshot.value.platform === "unsupported"),
);
function updateTerminalField(field: TerminalField, value: string) {
  terminalDraft[field] = value;
  markTerminalDirty(field);
}
function selectTab(tab: Tab) {
  active.value = tab;
  if (tab === "git" && !git.snapshot.value && !git.state.loadError) {
    void git.reload();
  }
}
function onTabKey(event: KeyboardEvent, index: number) {
  let target = index;
  if (event.key === "ArrowRight") {
    target = (index + 1) % tabs.length;
  } else if (event.key === "ArrowLeft") {
    target = (index + tabs.length - 1) % tabs.length;
  } else if (event.key === "Home") {
    target = 0;
  } else if (event.key === "End") {
    target = tabs.length - 1;
  } else {
    return;
  }
  event.preventDefault();
  const tab = tabs[target]!;
  selectTab(tab.key);
  document.getElementById(`tab-${tab.key}`)?.focus();
}
function reloadCurrent() {
  if (active.value === "git") {
    void git.reload();
  } else {
    void reload(active.value);
  }
}
function saveCurrent() {
  if (active.value === "git") {
    void git.save();
  } else {
    void save(active.value);
  }
}
onMounted(() => {
  void reload();
});
</script>

<template>
  <main class="app-shell">
    <header class="app-header">
      <div class="brand-mark" aria-hidden="true">↗</div>
      <div class="header-copy">
        <p class="eyebrow">PROXY MANAGER</p>
        <h1>代理设置</h1>
      </div>
      <span class="platform-badge">{{
        snapshot?.platform === "macos"
          ? "macOS"
          : snapshot?.platform === "windows"
            ? "Windows"
            : "本机配置"
      }}</span>
    </header>
    <p class="header-description">分开管理，各自保存。配置写入后，请另行确认网络连通性。</p>

    <div class="tabs" role="tablist" aria-label="代理类型">
      <button
        v-for="(tab, index) in tabs"
        :id="`tab-${tab.key}`"
        :key="tab.key"
        type="button"
        role="tab"
        :aria-selected="active === tab.key"
        :aria-controls="`panel-${tab.key}`"
        :tabindex="active === tab.key ? 0 : -1"
        @click="selectTab(tab.key)"
        @keydown="onTabKey($event, index)"
      >
        <span>{{ tab.label }}</span><small>{{ tab.detail }}</small>
        <span
          v-if="tab.key === 'git' ? git.dirty.value : dirty[tab.key]"
          class="dirty-dot"
          aria-label="有未保存修改"
        ></span>
      </button>
    </div>

    <div class="workspace">
      <div class="toolbar">
        <p class="read-state">
          <span :class="['state-dot', { pending: isDirty }]"></span
          >{{ isDirty ? "有未保存修改" : "当前配置"
          }}<span v-if="current.lastReadAt" class="read-time">
            · {{ current.lastReadAt }} 读取</span
          >
        </p>
        <button
          type="button"
          class="secondary-button"
          :disabled="current.loading || current.saving"
          @click="reloadCurrent"
        >
          {{ current.loading ? "正在读取…" : "重新读取" }}
        </button>
      </div>
      <p v-if="current.loading" class="operation-status" role="status">正在读取当前页配置…</p>
      <div v-if="current.loadError" class="global-error" role="alert">{{ current.loadError }}</div>

      <form @submit.prevent="saveCurrent">
        <section
          id="panel-terminal"
          v-show="active === 'terminal'"
          role="tabpanel"
          aria-labelledby="tab-terminal"
          tabindex="0"
        >
          <TerminalProxyForm
            v-if="snapshot"
            :draft="terminalDraft"
            :snapshot="snapshot.terminal"
            :disabled="pages.terminal.loading || pages.terminal.saving"
            @field-input="updateTerminalField"
          />
        </section>
        <section
          id="panel-system"
          v-show="active === 'system'"
          role="tabpanel"
          aria-labelledby="tab-system"
          tabindex="0"
        >
          <SystemProxyForm
            v-if="snapshot"
            :draft="systemDraft"
            :snapshot="snapshot.system"
            :platform="snapshot.platform"
            :errors="systemErrors"
            :submit-count="pages.system.submitCount"
            :disabled="pages.system.loading || pages.system.saving"
            @change="markSystemDirty"
            @select-target="selectTarget"
          />
        </section>
        <section
          id="panel-git"
          v-show="active === 'git'"
          role="tabpanel"
          aria-labelledby="tab-git"
          tabindex="0"
        >
          <GitProxyForm
            v-if="git.snapshot.value"
            :draft="git.draft"
            :snapshot="git.snapshot.value"
            :disabled="git.state.loading || git.state.saving"
            :error="git.state.submitCount ? git.error.value : ''"
            @mode-change="git.setMode"
            @proxy-input="git.setProxy"
          />
        </section>
        <div v-if="canSave" class="save-bar">
          <p class="hint">
            {{ current.saving ? "正在写入并回读，请稍候…" : "仅保存当前页，其他页草稿不受影响。" }}
          </p>
          <button
            class="primary-button"
            type="submit"
            :disabled="current.loading || current.saving"
          >
            {{ current.saving ? "正在保存…" : "保存当前设置" }}
          </button>
        </div>
      </form>
      <div v-if="current.saveError" class="global-error" role="alert">{{ current.saveError }}</div>
      <ApplyReport
        v-if="active !== 'git' && pages[active].report"
        :report="pages[active].report!"
      />
      <section
        v-if="active === 'git' && git.report.value"
        class="apply-report"
        :data-status="git.report.value.status"
        :role="
          git.report.value.status === 'failed' || git.report.value.status === 'partial'
            ? 'alert'
            : 'status'
        "
      >
        <h2>
          {{
            git.report.value.status === "applied"
              ? "已保存并回读"
              : git.report.value.status === "unchanged"
                ? "配置无变化"
                : "保存未完成"
          }}
        </h2>
        <p>{{ maskSensitiveText(git.report.value.message) }}</p>
      </section>
    </div>
    <footer class="app-footer">本机代理配置工具 <span>终端 · 系统 · Git</span></footer>
  </main>
</template>

<style>
:root {
  font-family: "Segoe UI", "PingFang SC", "Microsoft YaHei", sans-serif;
  font-size: 14px;
  line-height: 1.5;
  color: #243449;
  background: #f1f4f8;
  font-synthesis: none;
  text-rendering: optimizeLegibility;
  -webkit-font-smoothing: antialiased;
}
* {
  box-sizing: border-box;
}
body {
  margin: 0;
  min-width: 360px;
}
button,
input,
select,
textarea {
  font: inherit;
}
button,
input,
select,
textarea {
  border: 1px solid #b9c5d4;
  border-radius: 7px;
}
input,
select,
textarea {
  width: 100%;
  padding: 9px 11px;
  color: #243449;
  background: #fff;
}
input,
select {
  min-height: 39px;
}
textarea {
  resize: vertical;
}
button {
  padding: 8px 14px;
  color: #344c68;
  background: #fff;
  cursor: pointer;
}
button:hover:not(:disabled) {
  border-color: #668db8;
  background: #f1f6fc;
}
input:focus-visible,
select:focus-visible,
textarea:focus-visible,
button:focus-visible,
summary:focus-visible,
[role="tabpanel"]:focus-visible {
  outline: 3px solid #8cb8ee;
  outline-offset: 3px;
}
input[aria-invalid="true"],
select[aria-invalid="true"] {
  border-color: #bd3930;
  background: #fff8f7;
}
button:disabled,
fieldset:disabled input,
fieldset:disabled select,
fieldset:disabled textarea {
  cursor: not-allowed;
  opacity: 0.6;
}
.app-shell {
  width: min(1000px, 100%);
  margin: 0 auto;
  padding: 28px 32px 16px;
}
.app-header {
  display: flex;
  gap: 12px;
  align-items: center;
}
.brand-mark {
  display: grid;
  place-items: center;
  width: 44px;
  height: 44px;
  border-radius: 12px;
  background: #244e80;
  color: #fff;
  font-size: 28px;
  font-weight: 600;
}
.header-copy {
  flex: 1;
}
.eyebrow {
  margin: 0 0 1px;
  color: #60748d;
  font-size: 10px;
  font-weight: 700;
  letter-spacing: 0.14em;
}
.app-header h1 {
  margin: 0;
  color: #1b2e47;
  font-size: 22px;
  font-weight: 700;
  letter-spacing: 0.02em;
}
.platform-badge {
  padding: 4px 10px;
  border: 1px solid #d5dfeb;
  border-radius: 20px;
  color: #536982;
  background: #e8eef6;
  font-size: 12px;
}
.header-description {
  margin: 14px 0 22px;
  color: #66778b;
  font-size: 13px;
}
.tabs {
  display: flex;
  gap: 5px;
  border-bottom: 1px solid #d6e0ec;
}
.tabs button {
  position: relative;
  display: flex;
  align-items: baseline;
  gap: 8px;
  padding: 12px 20px 14px;
  border: 0;
  border-radius: 8px 8px 0 0;
  color: #64758a;
  background: transparent;
  font-weight: 600;
}
.tabs button[aria-selected="true"] {
  color: #22558e;
  background: #fff;
  font-weight: 600;
}
.tabs button[aria-selected="true"]::after {
  content: "";
  position: absolute;
  inset: auto 0 0;
  border-bottom: 3px solid #326aab;
}
.tabs small {
  font-size: 11px;
  font-weight: 400;
  color: #6a7b90;
}
.dirty-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: #bc7917;
}
.workspace {
  padding: 18px 24px 24px;
  border: 1px solid #d6e0ec;
  border-top: 0;
  border-radius: 0 0 12px 12px;
  background: #fff;
  box-shadow: 0 5px 20px #20374f06;
}
.toolbar {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 12px;
  padding-bottom: 16px;
  margin-bottom: 16px;
  border-bottom: 1px solid #edf0f5;
}
.read-state {
  display: flex;
  align-items: center;
  gap: 7px;
  margin: 0;
  font-size: 12px;
  color: #5b7088;
}
.state-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: #3f876a;
}
.state-dot.pending {
  background: #bc7917;
}
.read-time {
  color: #738399;
}
.secondary-button {
  font-size: 12px;
  padding: 6px 11px;
}
.proxy-section {
  min-width: 0;
  margin: 0;
  padding: 0;
  border: 0;
}
.proxy-section > legend {
  margin-bottom: 6px;
  padding: 0;
  font-size: 16px;
  font-weight: 650;
  color: #223953;
}
.section-intro,
.hint {
  font-size: 12px;
  line-height: 1.65;
  color: #64758a;
}
.section-intro {
  margin: 0 0 14px;
}
.terminal-fields {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 4px 18px;
  margin: 16px 0 0;
}
.field {
  min-width: 0;
  margin-bottom: 14px;
}
.field label {
  display: block;
  margin-bottom: 6px;
  font-size: 13px;
  font-weight: 600;
}
.field-error {
  margin: 5px 0 0;
  color: #ae2920;
  font-size: 12px;
}
.field .hint {
  margin: 5px 0 0;
}
.protocol-grid {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 12px;
  margin-bottom: 14px;
}
.protocol-group {
  min-width: 0;
  padding: 12px 14px 0;
  border: 1px solid #e2e8f1;
  border-radius: 8px;
  background: #f8fafc;
}
.protocol-group h3,
.snapshot-columns h3 {
  margin: 0 0 10px;
  font-size: 13px;
  font-weight: 650;
  color: #405e80;
}
.snapshot-columns {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 16px;
  margin-top: 12px;
}
.snapshot-columns section {
  min-width: 0;
}
.value-list {
  display: grid;
  grid-template-columns: minmax(90px, auto) minmax(0, 1fr);
  gap: 7px 10px;
  margin: 0;
  font-size: 12px;
}
.value-list dt {
  font-weight: 600;
}
.value-list dd {
  min-width: 0;
  margin: 0;
  overflow-wrap: anywhere;
  color: #64758a;
}
.details-panel {
  margin-top: 14px;
  padding: 11px 14px;
  border: 1px solid #e2e8f1;
  border-radius: 8px;
  background: #f8fafc;
}
.details-panel summary {
  cursor: pointer;
  font-size: 12px;
  font-weight: 600;
  color: #4c6685;
}
.details-panel p:last-child {
  margin-bottom: 0;
}
.notice {
  margin: 12px 0 0;
  padding: 10px 12px;
  border-left: 3px solid #6087b1;
  border-radius: 3px;
  background: #eef4fb;
  color: #49617e;
  font-size: 12px;
  overflow-wrap: anywhere;
}
.warning-notice,
.warnings {
  color: #845213;
  background: #fff7e9;
}
.warning-notice {
  border-left-color: #c38c32;
}
.warnings {
  margin: 12px 0 0;
  padding: 10px 12px 10px 28px;
  border-radius: 6px;
  font-size: 12px;
  overflow-wrap: anywhere;
}
.target-field {
  max-width: 380px;
}
.toggle-field {
  display: flex;
  align-items: center;
  gap: 8px;
  margin: 12px 0 16px;
  font-weight: 600;
  font-size: 13px;
}
.toggle-field input {
  width: 16px;
  min-height: 16px;
  height: 16px;
  margin: 0;
  accent-color: #2f65a1;
}
.save-bar {
  position: sticky;
  bottom: 0;
  z-index: 1;
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 16px;
  margin-top: 18px;
  padding: 14px 0 0;
  border-top: 1px solid #e8edf3;
  background: #fff;
}
.save-bar .hint {
  margin: 0;
}
.primary-button {
  flex: 0 0 auto;
  border-color: #2b609c;
  color: #fff;
  background: #2b609c;
  font-weight: 600;
  box-shadow: 0 2px 3px #294c7520;
}
.primary-button:hover:not(:disabled) {
  color: #fff;
  border-color: #214f83;
  background: #214f83;
}
.operation-status {
  padding: 9px 12px;
  border-radius: 6px;
  color: #315d8a;
  background: #edf4fc;
  font-size: 12px;
}
.global-error {
  margin: 12px 0;
  padding: 10px 12px;
  border: 1px solid #edc4bf;
  border-radius: 7px;
  color: #9e2b23;
  background: #fff5f3;
  font-size: 13px;
  overflow-wrap: anywhere;
}
.apply-report {
  margin-top: 16px;
  padding: 12px 14px;
  border: 1px solid #cddfd5;
  border-radius: 8px;
  background: #f6faf8;
}
.apply-report[data-status="failed"],
.apply-report[data-status="partial"] {
  border-color: #edc4bf;
  background: #fff7f5;
}
.apply-report h2 {
  margin: 0;
  font-size: 14px;
}
.apply-report p {
  margin: 6px 0 0;
  font-size: 12px;
  overflow-wrap: anywhere;
}
.section-result {
  margin-top: 8px;
}
.section-result h3 {
  margin: 0;
  font-size: 12px;
}
.app-footer {
  display: flex;
  justify-content: space-between;
  margin-top: 14px;
  color: #7a899b;
  font-size: 11px;
}
@media (max-width: 800px) {
  .app-shell {
    padding: 22px 22px 14px;
  }
  .workspace {
    padding: 16px 18px 20px;
  }
  .tabs button {
    padding: 12px 16px;
  }
  .tabs small {
    display: none;
  }
  .protocol-grid {
    gap: 9px;
  }
  .protocol-group {
    padding: 10px 10px 0;
  }
}
@media (max-width: 560px) {
  .app-shell {
    padding: 18px 12px;
  }
  .workspace {
    padding: 14px;
  }
  .terminal-fields,
  .protocol-grid,
  .snapshot-columns {
    grid-template-columns: 1fr;
  }
  .read-time {
    display: none;
  }
  .save-bar .hint {
    max-width: 160px;
  }
}
</style>
