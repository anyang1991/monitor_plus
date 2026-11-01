<script setup lang="ts">
import { computed, ref, onMounted, onUnmounted } from "vue";
import type { QuotaSnapshot, LimitEntry } from "../types";

const props = defineProps<{ snap: QuotaSnapshot | null }>();

const now = ref(Date.now());
let timer: number | undefined;
onMounted(() => {
  timer = window.setInterval(() => (now.value = Date.now()), 1000);
});
onUnmounted(() => window.clearInterval(timer));

function levelLabel(level: string | null): string {
  switch (level) {
    case "max": return "Max";
    case "pro": return "Pro";
    case "lite": return "Lite";
    case "start": return "Start";
    default: return level ?? "";
  }
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

function color(pct: number): string {
  if (pct >= 90) return "#f56c6c";
  if (pct >= 70) return "#e6a23c";
  return "#67c23a";
}

const rows = computed(() => {
  const s = props.snap;
  if (!s) return [];
  const out: { key: string; label: string; entry: LimitEntry }[] = [];
  if (s.five_hour) out.push({ key: "5h", label: "5 小时限额", entry: s.five_hour });
  if (s.weekly) out.push({ key: "week", label: "每周限额", entry: s.weekly });
  return out;
});
</script>

<template>
  <div class="glm-panel">
    <div class="glm-header">
      <span class="glm-title">GLM Coding Plan</span>
      <span v-if="snap?.level" class="level-badge">{{ levelLabel(snap.level) }}</span>
      <span v-if="snap?.key_source === 'zcode'" class="key-src" title="API key 自动读取自本机 ZCode">ZCode</span>
    </div>

    <div v-if="!snap" class="glm-status">正在获取…</div>
    <div v-else-if="!snap.ok" class="glm-status err" :title="snap.error ?? ''">
      {{ snap.error ?? "获取失败" }}
    </div>
    <template v-else>
      <div v-for="row in rows" :key="row.key" class="quota-row">
        <div class="quota-line">
          <span class="quota-label">{{ row.label }}</span>
          <span
            class="quota-pct"
            :style="{ color: color(row.entry.percentage) }"
            :title="`已用 ${row.entry.percentage}%，剩余 ${Math.max(0, 100 - row.entry.percentage)}%`"
          >
            已用 {{ row.entry.percentage }}%
          </span>
        </div>
        <div class="bar">
          <div
            class="bar-fill"
            :style="{ width: Math.min(100, row.entry.percentage) + '%', background: color(row.entry.percentage) }"
          />
        </div>
        <div class="quota-sub">
          <span>{{ countdown(row.entry.next_reset_ms) }}</span>
          <span v-if="row.entry.usage != null" class="quota-usage">
            {{ Math.round(row.entry.usage).toLocaleString() }} credits
          </span>
        </div>
      </div>
    </template>
  </div>
</template>

<style scoped>
.glm-panel {
  padding: 0 12px 6px;
}
.glm-header {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-bottom: 8px;
  padding-top: 8px;
}
.glm-title {
  font-size: 9px;
  font-weight: 700;
  color: #6b7488;
  letter-spacing: 0.08em;
  text-transform: uppercase;
}
.level-badge {
  font-size: 9px;
  font-weight: 700;
  padding: 1px 5px;
  border-radius: 4px;
  background: linear-gradient(135deg, #6b6bff33, #9b6bff33);
  border: 1px solid #7c7cff55;
  color: #b9b9ff;
  text-transform: uppercase;
}
.key-src {
  margin-left: auto;
  font-size: 9px;
  color: #5b6478;
}
.glm-status {
  font-size: 11px;
  color: #5b6478;
  padding: 4px 0 8px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.glm-status.err {
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
