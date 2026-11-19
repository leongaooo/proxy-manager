<script setup lang="ts">
import type { ApplyReport, ApplyStatus, SectionResult } from "../generated/bindings";
import { maskSensitiveText } from "../utils/sensitive";

defineProps<{
  report: ApplyReport;
}>();

const summaries: Record<ApplyStatus, string> = {
  applied: "设置已保存并回读。请另行验证代理服务器的实际连通性。",
  unchanged: "设置没有变化，已回读当前快照。",
  partial: "仅部分区块保存成功；失败区块的草稿已保留，请修正后单独核对再提交。",
  failed: "本次没有区块保存成功；草稿已保留，请按下列原因处理。",
};

function sectionMessage(section: SectionResult | null): string {
  if (!section) {
    return "该区块未参与本次保存。";
  }
  return maskSensitiveText(section.message || "该区块保存失败，请重新读取核对后再试。");
}
</script>

<template>
  <section class="apply-report" :data-status="report.status" aria-live="polite" aria-atomic="true">
    <h2>保存结果</h2>
    <p class="report-summary">{{ summaries[report.status] }}</p>
    <div v-if="report.terminal" class="section-result" :data-status="report.terminal.status">
      <h3>
        终端代理：{{
          report.terminal.status === "failed"
            ? "失败"
            : report.terminal.status === "applied"
              ? "已保存"
              : "无变化"
        }}
      </h3>
      <p>{{ sectionMessage(report.terminal) }}</p>
    </div>
    <div v-if="report.system" class="section-result" :data-status="report.system.status">
      <h3>
        系统代理：{{
          report.system.status === "failed"
            ? "失败"
            : report.system.status === "applied"
              ? "已保存"
              : "无变化"
        }}
      </h3>
      <p>{{ sectionMessage(report.system) }}</p>
    </div>
  </section>
</template>
