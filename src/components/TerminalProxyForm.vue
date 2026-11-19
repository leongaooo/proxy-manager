<script setup lang="ts">
import type { TerminalSnapshot, TerminalValues } from "../generated/bindings";
import type { TerminalDraft, TerminalField } from "../composables/useProxySettings";
import { maskSensitiveText } from "../utils/sensitive";

defineProps<{
  draft: TerminalDraft;
  snapshot: TerminalSnapshot;
  disabled: boolean;
}>();

const emit = defineEmits<{
  fieldInput: [field: TerminalField, value: string];
}>();

const fields: { key: TerminalField; label: string }[] = [
  { key: "httpProxy", label: "HTTP_PROXY" },
  { key: "httpsProxy", label: "HTTPS_PROXY" },
  { key: "allProxy", label: "ALL_PROXY" },
  { key: "noProxy", label: "NO_PROXY" },
];

function updateField(field: TerminalField, event: Event) {
  emit("fieldInput", field, (event.target as HTMLInputElement).value);
}

function displayValue(values: TerminalValues, field: TerminalField): string {
  const value = values[field];
  return value === null || value === "" ? "未设置" : maskSensitiveText(value);
}
</script>

<template>
  <fieldset class="proxy-section" :disabled="disabled">
    <legend>终端代理</legend>
    <p class="section-intro">
      编辑持久值（{{ snapshot.persistenceLabel }}）。应用进程值和其他已运行终端的值仅用于核对。
    </p>
    <p id="terminal-privacy-hint" class="hint">
      含认证信息的 URL
      已脱敏。未修改的字段保存时保持原值；修改时需输入完整新值，留空表示删除该变量。
    </p>

    <div class="terminal-fields">
      <div v-for="field in fields" :key="field.key" class="field">
        <label :for="`terminal-${field.key}`">{{ field.label }}</label>
        <input
          :id="`terminal-${field.key}`"
          :name="`terminal-${field.key}`"
          type="text"
          autocomplete="off"
          :value="draft[field.key]"
          :aria-describedby="field.key === 'httpProxy' ? 'terminal-privacy-hint' : undefined"
          @input="updateField(field.key, $event)"
        />
      </div>
    </div>

    <div class="snapshot-columns">
      <section aria-labelledby="process-values-title">
        <h3 id="process-values-title">应用进程值</h3>
        <p class="hint">这是当前应用读取到的环境，不代表其他终端。</p>
        <dl class="value-list">
          <template v-for="field in fields" :key="field.key">
            <dt>{{ field.label }}</dt>
            <dd>{{ displayValue(snapshot.process, field.key) }}</dd>
          </template>
        </dl>
      </section>

      <section v-if="snapshot.launchd" aria-labelledby="launchd-values-title">
        <h3 id="launchd-values-title">macOS launchd 用户会话值</h3>
        <p class="hint">由图形界面应用继承的环境，与 Shell 持久值分开显示。</p>
        <dl class="value-list">
          <template v-for="field in fields" :key="field.key">
            <dt>{{ field.label }}</dt>
            <dd>{{ displayValue(snapshot.launchd, field.key) }}</dd>
          </template>
        </dl>
      </section>
    </div>

    <div class="notice">
      已运行的终端不能可靠热更新。保存后请打开新终端，或按所用 Shell
      的方式重载配置；保存成功不代表代理服务器可以连接。
    </div>

    <ul v-if="snapshot.warnings.length" class="warnings" aria-label="终端代理警告">
      <li v-for="warning in snapshot.warnings" :key="warning">
        {{ maskSensitiveText(warning) }}
      </li>
    </ul>
  </fieldset>
</template>
