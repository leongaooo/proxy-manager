<script setup lang="ts">
import { reactive } from "vue";
import type { Platform, SystemSnapshot } from "../generated/bindings";
import type { SystemDraft, SystemField, SystemProtocol } from "../composables/useProxySettings";
import { maskSensitiveText } from "../utils/sensitive";

const props = defineProps<{
  draft: SystemDraft;
  snapshot: SystemSnapshot;
  platform: Platform;
  errors: Partial<Record<SystemField, string>>;
  submitCount: number;
  disabled: boolean;
}>();

const emit = defineEmits<{
  change: [];
  selectTarget: [target: string];
}>();

const touched = reactive<Record<string, boolean>>({});
const protocols: { key: SystemProtocol; label: string }[] = [
  { key: "http", label: "HTTP" },
  { key: "https", label: "HTTPS" },
  { key: "socks", label: "SOCKS" },
];

function visibleError(field: SystemField): string {
  return touched[field] || props.submitCount > 0 ? (props.errors[field] ?? "") : "";
}

function selectTarget(event: Event) {
  emit("selectTarget", (event.target as HTMLSelectElement).value);
}
</script>

<template>
  <fieldset class="proxy-section" :disabled="disabled">
    <legend>手动系统代理</legend>
    <p class="section-intro">
      配置操作系统的手动代理。保存不会擅自清除 PAC
      等自动配置；存在冲突时请按警告到系统网络设置中处理。
    </p>

    <div v-if="platform === 'macos'" class="field target-field">
      <label for="system-target">macOS 网络服务</label>
      <select
        id="system-target"
        name="system-target"
        autocomplete="off"
        :value="draft.target"
        :aria-invalid="visibleError('target') ? 'true' : 'false'"
        :aria-describedby="visibleError('target') ? 'system-target-error' : undefined"
        @change="selectTarget"
        @blur="touched.target = true"
      >
        <option value="" disabled>请选择网络服务</option>
        <option v-for="target in snapshot.availableTargets" :key="target" :value="target">
          {{ target }}
        </option>
      </select>
      <p v-if="visibleError('target')" id="system-target-error" class="field-error">
        {{ visibleError("target") }}
      </p>
      <p v-if="!snapshot.availableTargets.length" class="hint">
        未读取到可用网络服务，请检查系统网络设置和读取权限。
      </p>
    </div>

    <label class="toggle-field" for="system-enabled">
      <input
        id="system-enabled"
        v-model="draft.enabled"
        name="system-enabled"
        type="checkbox"
        @change="emit('change')"
      />
      启用手动系统代理（关闭时保留服务器与绕过配置）
    </label>

    <div class="protocol-grid">
      <section
        v-for="protocol in protocols"
        :key="protocol.key"
        class="protocol-group"
        :aria-labelledby="`${protocol.key}-title`"
      >
        <h3 :id="`${protocol.key}-title`">{{ protocol.label }}</h3>
        <div class="field">
          <label :for="`system-${protocol.key}-host`">主机</label>
          <input
            :id="`system-${protocol.key}-host`"
            v-model="draft[protocol.key].host"
            :name="`system-${protocol.key}-host`"
            type="text"
            autocomplete="off"
            :aria-invalid="visibleError(`${protocol.key}.host`) ? 'true' : 'false'"
            :aria-describedby="
              visibleError(`${protocol.key}.host`) ? `system-${protocol.key}-host-error` : undefined
            "
            @input="emit('change')"
            @blur="touched[`${protocol.key}.host`] = true"
          />
          <p
            v-if="visibleError(`${protocol.key}.host`)"
            :id="`system-${protocol.key}-host-error`"
            class="field-error"
          >
            {{ visibleError(`${protocol.key}.host`) }}
          </p>
        </div>
        <div class="field">
          <label :for="`system-${protocol.key}-port`">端口</label>
          <input
            :id="`system-${protocol.key}-port`"
            v-model="draft[protocol.key].port"
            :name="`system-${protocol.key}-port`"
            type="text"
            inputmode="numeric"
            autocomplete="off"
            :aria-invalid="visibleError(`${protocol.key}.port`) ? 'true' : 'false'"
            :aria-describedby="
              visibleError(`${protocol.key}.port`) ? `system-${protocol.key}-port-error` : undefined
            "
            @input="emit('change')"
            @blur="touched[`${protocol.key}.port`] = true"
          />
          <p
            v-if="visibleError(`${protocol.key}.port`)"
            :id="`system-${protocol.key}-port-error`"
            class="field-error"
          >
            {{ visibleError(`${protocol.key}.port`) }}
          </p>
        </div>
      </section>
    </div>

    <div class="field">
      <label for="system-bypass">绕过代理（bypass）</label>
      <textarea
        id="system-bypass"
        v-model="draft.bypassText"
        name="system-bypass"
        autocomplete="off"
        rows="4"
        aria-describedby="system-bypass-hint"
        @input="emit('change')"
      />
      <p id="system-bypass-hint" class="hint">
        每行或用逗号分隔一个主机、域名或网段；保存时会去除空项并归一为数组。
      </p>
    </div>

    <div v-if="snapshot.autoConfig" class="notice warning-notice">
      检测到自动代理配置（PAC）：{{
        maskSensitiveText(snapshot.autoConfig)
      }}。此处仅提示冲突，不会自动清除。
    </div>

    <ul v-if="snapshot.warnings.length" class="warnings" aria-label="系统代理警告">
      <li v-for="warning in snapshot.warnings" :key="warning">
        {{ maskSensitiveText(warning) }}
      </li>
    </ul>
  </fieldset>
</template>
