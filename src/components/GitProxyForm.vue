<script setup lang="ts">
import { ref } from "vue";
import type { GitProxyMode, GitProxySnapshot } from "../generated/bindings";
import { maskSensitiveText } from "../utils/sensitive";
defineProps<{
  draft: { mode: GitProxyMode; proxy: string };
  snapshot: GitProxySnapshot;
  disabled: boolean;
  error: string;
}>();
const emit = defineEmits<{ modeChange: [mode: GitProxyMode]; proxyInput: [proxy: string] }>();
const reveal = ref(false);
const modes: { key: GitProxyMode; label: string; hint: string }[] = [
  { key: "custom", label: "指定代理", hint: "写入代理地址" },
  { key: "inherit", label: "继承环境", hint: "移除全局键" },
  { key: "disabled", label: "不使用代理", hint: "写入显式空值" },
];
</script>

<template>
  <fieldset class="proxy-section" :disabled="disabled || !snapshot.writable">
    <legend>Git 全局代理</legend>
    <p class="section-intro">为 Git 的 HTTP / HTTPS 请求配置同一个代理地址。</p>
    <div class="git-modes" role="radiogroup" aria-label="Git 代理方式">
      <label
        v-for="mode in modes"
        :key="mode.key"
        :class="['mode-option', { selected: draft.mode === mode.key }]"
      >
        <input
          type="radio"
          name="git-mode"
          :value="mode.key"
          :checked="draft.mode === mode.key"
          @change="emit('modeChange', mode.key)"
        />
        <span
          ><strong>{{ mode.label }}</strong
          ><small>{{ mode.hint }}</small></span
        >
      </label>
    </div>
    <div v-if="draft.mode === 'custom'" class="field git-address">
      <label for="git-proxy">代理地址</label>
      <div class="address-control">
        <input
          id="git-proxy"
          :type="reveal ? 'text' : 'password'"
          :value="draft.proxy"
          autocomplete="off"
          spellcheck="false"
          placeholder="http://127.0.0.1:7890"
          :aria-invalid="!!error"
          aria-describedby="git-privacy-hint"
          @input="emit('proxyInput', ($event.target as HTMLInputElement).value)"
        />
        <button type="button" :aria-pressed="reveal" @click="reveal = !reveal">
          {{ reveal ? "隐藏" : "显示" }}
        </button>
      </div>
      <p v-if="error" class="field-error" role="alert">{{ error }}</p>
      <p id="git-privacy-hint" class="hint">
        认证信息已脱敏。未编辑时保留原值；编辑时请填写完整地址。
      </p>
    </div>
    <p v-else class="notice">
      {{
        draft.mode === "inherit"
          ? "删除全局 http.proxy，由环境和其他 Git 配置决定代理。"
          : "将全局 http.proxy 设为空值，关闭该层级的代理。"
      }}
    </p>
  </fieldset>
  <p v-if="!snapshot.available" class="global-error" role="alert">
    未找到 Git。安装 Git 后重新读取。
  </p>
  <ul v-if="snapshot.warnings.length" class="warnings" aria-label="Git 配置警告">
    <li v-for="warning in snapshot.warnings" :key="warning">{{ maskSensitiveText(warning) }}</li>
  </ul>
  <details class="details-panel">
    <summary>适用范围与配置说明</summary>
    <p class="hint">
      仅管理当前用户的全局 http.proxy。URL 专用代理、remote 代理、仓库和 worktree
      配置可能覆盖此设置；不修改这些配置及非规范的 https.proxy。
    </p>
    <p class="hint">
      include、多来源或重复键需先在 Git
      配置中核对。保存前检查配置变化，保存后回读；写入与回读之间仍可能发生外部修改。保存成功不等于代理可用。
    </p>
  </details>
</template>

<style scoped>
.git-modes {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 12px;
  margin: 16px 0 20px;
}
.mode-option {
  display: flex;
  align-items: center;
  gap: 9px;
  padding: 13px 12px;
  border: 1px solid #dce4ee;
  border-radius: 8px;
  background: #f8fafc;
  cursor: pointer;
}
.mode-option.selected {
  border-color: #6891bf;
  background: #eef4fc;
  box-shadow: inset 0 0 0 1px #6891bf;
}
.mode-option input {
  width: 15px;
  height: 15px;
  min-height: 15px;
  margin: 0;
  accent-color: #2f65a1;
}
.mode-option strong {
  display: block;
  font-size: 13px;
  font-weight: 600;
}
.mode-option small {
  display: block;
  margin-top: 3px;
  font-size: 11px;
  color: #66798f;
}
.address-control {
  display: flex;
  gap: 8px;
}
.address-control input {
  min-width: 0;
}
.address-control button {
  flex: 0 0 auto;
  font-size: 12px;
}
@media (max-width: 560px) {
  .git-modes {
    grid-template-columns: 1fr;
  }
}
</style>
