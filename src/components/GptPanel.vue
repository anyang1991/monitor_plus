<script setup lang="ts">
import { computed, ref, onMounted, onUnmounted } from "vue";
import type { GptSnapshot, GptWindowUsage, TiboSnapshot } from "../types";

const props = defineProps<{ snap: GptSnapshot | null; tibo: TiboSnapshot | null }>();

const resetRow = computed(() => {
  const t = props.tibo;
  if (!t) return null;
  if (!t.ok) return { label: "Tibo 重置官宣", value: t.error ?? "获取中…", err: true, tip: t.error ?? "" };
  // 预告回退：无确定日期但 Tibo 预告了近期重置（与收起条一致）
  if (!t.next_date_full && t.preview_note) {
    return {
      label: "下次重置",
      value: "预告 1-2 天内",
      err: false,
      color: "#e6a23c",
      tip: `${t.preview_note}\n${t.summary ?? ""}`,
    };
  }
  if (!t.next_date_full) return { label: "下次重置", value: "无计划", err: false, tip: "Tibo 尚未官宣下一次重置" };
  // 倒计时到该日期结束（UTC，近似 PT 当天结束）
  const target = Date.parse(t.next_date_full + "T23:59:59Z");
  const diff = target - now.value;
  const h = Math.max(0, Math.floor(diff / 3600000));
  let value: string;
  if (diff <= 0) value = "进行中";
  else {
    if (h >= 48) value = `还有 ${Math.floor(h / 24)} 天`;
    else if (h >= 1) value = `还有 ${h} 小时`;
    else value = `还有 ${Math.max(1, Math.round(diff / 60000))} 分钟`;
  }
  return {
    label: "下次重置",
    value,
    err: false,
    tip: `Tibo 官宣重置日 ${t.next_date_full}`,
    color: value === "进行中" ? "#f56c6c" : h >= 48 ? "#67c23a" : h >= 24 ? "#e6a23c" : "#f56c6c",
  };
});

const now = ref(Date.now());
let timer: number | undefined;
onMounted(() => {
  timer = window.setInterval(() => (now.value = Date.now()), 1000);
});
onUnmounted(() => window.clearInterval(timer));

function color(pct: number): string {
  if (pct >= 90) return "#f56c6c";
  if (pct >= 70) return "#e6a23c";
  return "#67c23a";
}

function countdown(resetMs: number | null): string {
  if (!resetMs) return "";
  const diff = resetMs - now.value;
  if (diff <= 0) return "即将重置";
  const mins = Math.floor(diff / 60000);
  const h = Math.floor(mins / 60);
  const m = mins % 60;
  if (h >= 24) {
    const d = Math.floor(h / 24);
    return `${d}天${h % 24}时后重置`;
  }
  return h > 0 ? `${h}时${m}分后重置` : `${m}分后重置`;
}

const rows = computed(() => {
  const s = props.snap;
  if (!s) return [];
  const out: { key: string; label: string; entry: GptWindowUsage }[] = [];
  if (s.five_hour) out.push({ key: "5h", label: "5 小时限额", entry: s.five_hour });
  if (s.weekly) out.push({ key: "week", label: "每周限额", entry: s.weekly });
  return out;
});
</script>

<template>
  <div v-if="snap?.configured" class="gpt-panel">
    <div class="gpt-header">
      <span class="gpt-title" :title="snap?.plan_debug ?? ''">GPT Codex</span>
      <span v-if="snap?.plan" class="plan-badge">{{ snap.plan }}</span>
      <span v-if="snap?.key_source === 'codex'" class="key-src" title="token 自动读取自本机 ~/.codex/auth.json">Codex</span>
    </div>
    <div
      v-if="snap?.ok && !snap?.plan && snap?.plan_debug"
      class="plan-debug"
      :title="snap.plan_debug"
    >
      套餐获取失败：{{ snap.plan_debug.slice(0, 90) }}
    </div>

    <div v-if="!snap?.ok" class="gpt-status err" :title="snap?.error ?? ''">
      {{ snap?.error ?? "正在获取…" }}
    </div>
    <template v-else>
      <div v-for="row in rows" :key="row.key" class="quota-row">
        <div class="quota-line">
          <span class="quota-label">{{ row.label }}</span>
          <span
            class="quota-pct"
            :style="{ color: color(row.entry.used_percent) }"
            :title="`已用 ${row.entry.used_percent}%，剩余 ${Math.max(0, 100 - row.entry.used_percent)}%`"
          >
            已用 {{ row.entry.used_percent }}%
          </span>
        </div>
        <div class="bar">
          <div
            class="bar-fill"
            :style="{ width: Math.min(100, row.entry.used_percent) + '%', background: color(row.entry.used_percent) }"
          />
        </div>
        <div class="quota-sub">
          <span>{{ countdown(row.entry.reset_at_ms) }}</span>
          <span v-if="snap?.credits_balance" class="quota-usage">${{ snap.credits_balance }} credits</span>
        </div>
      </div>
      <div v-if="resetRow" class="quota-row" :title="resetRow.tip">
        <div class="quota-line">
          <span class="quota-label">{{ resetRow.label }}</span>
          <span
            class="quota-pct"
            :style="{ color: resetRow.err ? '#f56c6c' : resetRow.color }"
          >{{ resetRow.value }}</span>
        </div>
        <div v-if="!resetRow.err && props.tibo?.next_date_full" class="quota-sub">
          <span class="quota-usage" :title="props.tibo?.signal_summary ?? ''">
            {{ props.tibo?.stale ? "数据可能过期" : "Tibo 官宣追踪" }}
          </span>
        </div>
      </div>
    </template>
  </div>
</template>

<style scoped>
.gpt-panel {
  padding: 0 12px 6px;
  border-top: 1px solid #ffffff0d;
  margin-top: 4px;
}
.gpt-header {
  display: flex;
  align-items: center;
  gap: 6px;
  padding-top: 8px;
  margin-bottom: 8px;
}
.gpt-title {
  font-size: 9px;
  font-weight: 700;
  color: #6b7488;
  letter-spacing: 0.08em;
  text-transform: uppercase;
}
.plan-badge {
  font-size: 9px;
  font-weight: 700;
  padding: 1px 5px;
  border-radius: 4px;
  background: #10a37f26;
  border: 1px solid #10a37f55;
  color: #4ade9d;
  text-transform: uppercase;
}
.plan-debug {
  font-size: 9px;
  color: #e6a23c;
  padding: 0 0 6px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  cursor: help;
}
.key-src {
  margin-left: auto;
  font-size: 9px;
  color: #5b6478;
}
.gpt-status {
  font-size: 11px;
  color: #5b6478;
  padding: 2px 0 8px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.gpt-status.err {
  color: #f56c6c;
}
.quota-row {
  margin-bottom: 8px;
}
.quota-line {
  display: flex;
  justify-content: space-between;
  font-size: 10px;
  margin-bottom: 3px;
}
.quota-label {
  color: #8a93a8;
}
.quota-pct {
  font-weight: 700;
  font-variant-numeric: tabular-nums;
}
.bar {
  height: 5px;
  border-radius: 3px;
  background: #ffffff14;
  overflow: hidden;
}
.bar-fill {
  height: 100%;
  border-radius: 3px;
  transition: width 0.6s ease, background 0.6s ease;
}
.quota-sub {
  display: flex;
  justify-content: space-between;
  font-size: 9px;
  color: #5b6478;
  margin-top: 2px;
  font-variant-numeric: tabular-nums;
}
</style>
