<script setup lang="ts">
import { onMounted } from "vue";
import ApplyReport from "./components/ApplyReport.vue";
import SystemProxyForm from "./components/SystemProxyForm.vue";
import TerminalProxyForm from "./components/TerminalProxyForm.vue";
import { useProxySettings, type TerminalField } from "./composables/useProxySettings";
import { maskSensitiveText } from "./utils/sensitive";

const {
  snapshot,
  report,
  loading,
  saving,
  loadError,
  saveError,
  submitCount,
  lastReadAt,
  terminalDraft,
  systemDraft,
  systemErrors,
  reload,
  selectTarget,
  saveAll,
  markTerminalDirty,
  markSystemDirty,
} = useProxySettings();

function updateTerminalField(field: TerminalField, value: string) {
  terminalDraft[field] = value;
  markTerminalDirty(field);
}

onMounted(() => {
  void reload();
});
</script>

<template>
  <main class="app-shell">
    <header class="app-header">
      <p class="eyebrow">网络环境核对与写入</p>
      <h1>代理设置</h1>
      <p>分别管理终端环境变量与操作系统手动代理。这里只保存配置，不测试代理服务器的可用性。</p>
    </header>

    <div class="toolbar">
      <button type="button" :disabled="loading || saving" @click="reload">
        {{ loading ? "正在读取" : "重新读取" }}
      </button>
      <p v-if="snapshot" class="platform-label">
        当前平台：{{
          snapshot.platform === "macos"
            ? "macOS"
            : snapshot.platform === "windows"
              ? "Windows"
              : "不支持的平台"
        }}
        <span v-if="lastReadAt"> · 最后读取：{{ lastReadAt }}</span>
      </p>
    </div>

    <p v-if="loading" class="operation-status" role="status" aria-live="polite">
      {{ snapshot ? "正在重新读取完整代理快照…" : "正在读取终端与系统代理设置…" }}
    </p>

    <div v-if="loadError" class="global-error" role="alert">
      <p>{{ loadError }}</p>
      <p>可修正权限或服务选择后，使用“重新读取”恢复；当前草稿不会因读取失败被清除。</p>
    </div>

    <template v-if="snapshot">
      <ul v-if="snapshot.warnings.length" class="warnings global-warnings" aria-label="全局警告">
        <li v-for="warning in snapshot.warnings" :key="warning">
          {{ maskSensitiveText(warning) }}
        </li>
      </ul>

      <p v-if="snapshot.platform === 'unsupported'" class="notice warning-notice">
        当前平台不支持手动系统代理写入。可查看已读取的信息，但不要提交系统代理更改。
      </p>

      <form class="settings-form" @submit.prevent="saveAll">
        <TerminalProxyForm
          :draft="terminalDraft"
          :snapshot="snapshot.terminal"
          :disabled="loading || saving"
          @field-input="updateTerminalField"
        />
        <SystemProxyForm
          :draft="systemDraft"
          :snapshot="snapshot.system"
          :platform="snapshot.platform"
          :errors="systemErrors"
          :submit-count="submitCount"
          :disabled="loading || saving"
          @change="markSystemDirty"
          @select-target="selectTarget"
        />

        <div class="save-bar">
          <button class="primary-button" type="submit" :disabled="loading || saving">
            {{ saving ? "正在保存…" : "保存全部更改" }}
          </button>
          <p class="hint">保存期间不能重复提交。成功后以回读快照为准，并需另行验证网络连通性。</p>
        </div>
      </form>

      <p v-if="saving" class="operation-status" role="status" aria-live="polite">
        正在保存并回读终端与系统代理，请等待结果…
      </p>
      <div v-if="saveError" class="global-error" role="alert">
        {{ saveError }}
      </div>
      <ApplyReport v-if="report" :report="report" />
    </template>
  </main>
</template>

<style>
:root {
  font-family: "Segoe UI", "PingFang SC", "Microsoft YaHei", sans-serif;
  color: #182230;
  background: #eef2f6;
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
  min-height: 100vh;
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
  border: 1px solid #a8b4c3;
  border-radius: 6px;
}

input,
select,
textarea {
  width: 100%;
  padding: 9px 11px;
  color: #182230;
  background: #fff;
}

textarea {
  resize: vertical;
}

input:focus-visible,
select:focus-visible,
textarea:focus-visible,
button:focus-visible {
  outline: 3px solid #7db4ef;
  outline-offset: 2px;
}

input[aria-invalid="true"],
select[aria-invalid="true"] {
  border-color: #b42318;
  background: #fff7f6;
}

button {
  padding: 9px 16px;
  color: #20344d;
  background: #fff;
  cursor: pointer;
}

button:hover:not(:disabled) {
  background: #edf4fb;
}

button:disabled,
fieldset:disabled input,
fieldset:disabled select,
fieldset:disabled textarea {
  cursor: not-allowed;
  opacity: 0.65;
}

.app-shell {
  width: min(1120px, 100%);
  margin: 0 auto;
  padding: 28px 24px 48px;
}

.app-header {
  margin-bottom: 20px;
}

.app-header h1 {
  margin: 2px 0 8px;
  font-size: 30px;
}

.app-header p {
  margin: 0;
  max-width: 760px;
  color: #526174;
}

.app-header .eyebrow {
  color: #315b85;
  font-size: 13px;
  font-weight: 700;
  letter-spacing: 0.08em;
}

.toolbar {
  display: flex;
  align-items: center;
  gap: 16px;
  margin-bottom: 16px;
}

.platform-label {
  margin: 0;
  color: #526174;
}

.settings-form {
  display: grid;
  gap: 18px;
}

.proxy-section {
  min-width: 0;
  margin: 0;
  padding: 22px;
  border: 1px solid #d3dce6;
  border-radius: 10px;
  background: #fff;
  box-shadow: 0 2px 8px rgb(30 48 68 / 5%);
}

.proxy-section > legend {
  padding: 0 8px;
  font-size: 20px;
  font-weight: 700;
}

.section-intro,
.hint {
  color: #5a697a;
  font-size: 14px;
  line-height: 1.6;
}

.section-intro {
  margin-top: 0;
}

.terminal-fields {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 16px;
  margin: 20px 0;
}

.field {
  min-width: 0;
  margin-bottom: 14px;
}

.field label {
  display: block;
  margin-bottom: 6px;
  font-size: 14px;
  font-weight: 650;
}

.field-error {
  margin: 6px 0 0;
  color: #a61b13;
  font-size: 14px;
}

.snapshot-columns,
.protocol-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 16px;
}

.snapshot-columns section,
.protocol-group {
  min-width: 0;
  padding: 16px;
  border: 1px solid #e0e6ed;
  border-radius: 8px;
  background: #f8fafc;
}

.snapshot-columns h3,
.protocol-group h3 {
  margin: 0 0 8px;
  font-size: 16px;
}

.value-list {
  display: grid;
  grid-template-columns: minmax(100px, auto) minmax(0, 1fr);
  gap: 8px 12px;
  margin: 12px 0 0;
  font-size: 13px;
}

.value-list dt {
  font-weight: 650;
}

.value-list dd {
  min-width: 0;
  margin: 0;
  overflow-wrap: anywhere;
  color: #526174;
}

.notice {
  margin-top: 16px;
  padding: 12px 14px;
  border-left: 4px solid #467ba9;
  color: #364a60;
  background: #edf5fb;
  line-height: 1.6;
}

.warning-notice {
  border-left-color: #b54708;
  color: #713b12;
  background: #fff4e5;
}

.warnings {
  margin: 16px 0 0;
  padding: 12px 14px 12px 32px;
  border-radius: 6px;
  color: #713b12;
  background: #fff4e5;
  line-height: 1.6;
}

.global-warnings {
  margin-bottom: 18px;
}

.target-field {
  max-width: 480px;
}

.toggle-field {
  display: flex;
  align-items: center;
  gap: 9px;
  margin: 10px 0 20px;
  font-weight: 650;
}

.toggle-field input {
  width: 18px;
  height: 18px;
  margin: 0;
}

.protocol-grid {
  grid-template-columns: repeat(3, minmax(0, 1fr));
  margin-bottom: 16px;
}

.save-bar {
  display: flex;
  align-items: center;
  gap: 16px;
  padding: 4px 0;
}

.save-bar .hint {
  margin: 0;
}

.primary-button {
  flex: 0 0 auto;
  border-color: #245f96;
  color: #fff;
  background: #245f96;
  font-weight: 700;
}

.primary-button:hover:not(:disabled) {
  background: #194c7b;
}

.operation-status {
  padding: 12px 14px;
  border-radius: 6px;
  color: #294c6d;
  background: #e4eff9;
}

.global-error {
  margin: 16px 0;
  padding: 12px 14px;
  border: 1px solid #e1aaa5;
  border-radius: 6px;
  color: #8d1e17;
  background: #fff1f0;
  line-height: 1.6;
}

.global-error p {
  margin: 0 0 6px;
}

.global-error p:last-child {
  margin-bottom: 0;
}

.apply-report {
  margin-top: 20px;
  padding: 20px;
  border: 1px solid #b9c9d9;
  border-radius: 10px;
  background: #fff;
}

.apply-report h2 {
  margin: 0;
  font-size: 20px;
}

.report-summary {
  color: #46586c;
}

.section-result {
  padding: 12px 14px;
  border-left: 4px solid #357a54;
  background: #edf8f1;
}

.section-result + .section-result {
  margin-top: 10px;
}

.section-result[data-status="failed"] {
  border-left-color: #b42318;
  background: #fff1f0;
}

.section-result h3,
.section-result p {
  margin: 0;
}

.section-result h3 {
  margin-bottom: 5px;
  font-size: 15px;
}

@media (max-width: 800px) {
  .protocol-grid,
  .terminal-fields,
  .snapshot-columns {
    grid-template-columns: 1fr;
  }

  .save-bar {
    align-items: flex-start;
    flex-direction: column;
  }
}

@media (max-width: 520px) {
  .app-shell {
    padding: 20px 14px 36px;
  }

  .proxy-section {
    padding: 16px;
  }
}
</style>
