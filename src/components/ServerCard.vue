<script setup lang="ts">
import { computed } from "vue";
import type { ServerUpdate } from "../types";

const props = defineProps<{ update: ServerUpdate | null; name: string }>();

const m = computed(() => props.update?.metrics ?? null);
const online = computed(() => props.update?.online ?? false);

function color(pct: number): string {
  if (pct >= 90) return "#f56c6c";
  if (pct >= 70) return "#e6a23c";
  return "#67c23a";
}

function tempColor(t: number): string {
  if (t >= 85) return "#f56c6c";
  if (t >= 70) return "#e6a23c";
  return "#8a93a8";
}

function shortName(name: string): string {
  return name.length > 18 ? name.slice(0, 17) + "…" : name;
}

function gpuLabel(name: string): string {
  return name
    .replace("NVIDIA GeForce RTX", "RTX")
    .replace("NVIDIA A", "A")
    .replace("GB", "");
}
</script>

<template>
  <div class="server-card" :class="{ off: !online }">
    <div class="srv-line">
      <span class="dot" :class="online ? 'ok' : 'bad'" />
      <span class="srv-name" :title="name">{{ shortName(name) }}</span>
      <span v-if="online && update?.latency_ms != null" class="latency">{{ update.latency_ms }}ms</span>
      <span v-else-if="!online" class="latency err" :title="update?.error ?? ''">离线</span>
    </div>

    <template v-if="online && m">
      <!-- CPU -->
      <div class="metric-line">
        <span class="metric-label">CPU</span>
        <div class="bar slim">
          <div class="bar-fill" :style="{ width: Math.min(100, m.cpu_percent) + '%', background: color(m.cpu_percent) }" />
        </div>
        <span class="metric-val" :style="{ color: color(m.cpu_percent) }">{{ m.cpu_percent }}%</span>
      </div>
      <!-- 内存 -->
      <div v-if="m.mem_total_gb" class="metric-line">
        <span class="metric-label">内存</span>
        <div class="bar slim">
          <div
            class="bar-fill"
            :style="{
              width: Math.min(100, (m.mem_used_gb! / m.mem_total_gb) * 100) + '%',
              background: color((m.mem_used_gb! / m.mem_total_gb) * 100),
            }"
          />
        </div>
        <span class="metric-val dim">{{ m.mem_used_gb }}/{{ m.mem_total_gb }}G</span>
      </div>
      <!-- GPU 列表 -->
      <div v-for="g in m.gpus" :key="g.index" class="gpu-block">
        <div class="gpu-line">
          <span class="gpu-name" :title="g.name">{{ gpuLabel(g.name) }}</span>
          <span v-if="g.temp_c != null" class="temp" :style="{ color: tempColor(g.temp_c) }">{{ g.temp_c }}°C</span>
        </div>
        <div class="metric-line tight">
          <span class="metric-label">算力</span>
          <div class="bar slim">
            <div class="bar-fill" :style="{ width: Math.min(100, g.util) + '%', background: color(g.util) }" />
          </div>
          <span class="metric-val" :style="{ color: color(g.util) }">{{ g.util }}%</span>
        </div>
        <div class="metric-line tight">
          <span class="metric-label">显存</span>
          <div class="bar slim">
            <div
              class="bar-fill"
              :style="{
                width: Math.min(100, (g.mem_used_mb / g.mem_total_mb) * 100) + '%',
                background: color((g.mem_used_mb / g.mem_total_mb) * 100),
              }"
            />
          </div>
          <span class="metric-val dim">
            {{ (g.mem_used_mb / 1024).toFixed(1) }}/{{ (g.mem_total_mb / 1024).toFixed(0) }}G
          </span>
        </div>
      </div>
      <div v-if="m.gpus.length === 0" class="gpu-none">无 GPU 或 nvidia-smi 不可用</div>
    </template>
  </div>
</template>

<style scoped>
.server-card {
  background: #ffffff08;
  border: 1px solid #ffffff12;
  border-radius: 9px;
  padding: 8px 10px;
}
.server-card.off {
  opacity: 0.55;
}
.srv-line {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-bottom: 4px;
}
.dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  flex-shrink: 0;
}
.dot.ok {
  background: #67c23a;
  box-shadow: 0 0 4px #67c23a88;
}
.dot.bad {
  background: #f56c6c;
}
.srv-name {
  font-size: 11px;
  font-weight: 600;
  color: #d0d6e4;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.latency {
  margin-left: auto;
  font-size: 9px;
  color: #5b6478;
  font-variant-numeric: tabular-nums;
}
.latency.err {
  color: #f56c6c;
}
.metric-line {
  display: flex;
  align-items: center;
  gap: 6px;
  margin: 3px 0;
}
.metric-line.tight {
  margin: 2px 0;
}
.metric-label {
  width: 26px;
  font-size: 9px;
  color: #5b6478;
  flex-shrink: 0;
}
.bar.slim {
  flex: 1;
  height: 4px;
  border-radius: 2px;
  background: #ffffff14;
  overflow: hidden;
}
.bar-fill {
  height: 100%;
  transition: width 0.5s ease;
}
.metric-val {
  width: 52px;
  text-align: right;
  font-size: 9px;
  font-weight: 600;
  font-variant-numeric: tabular-nums;
}
.metric-val.dim {
  color: #8a93a8;
  font-weight: 400;
}
.gpu-block {
  margin: 4px 0 2px;
  padding-left: 8px;
  border-left: 2px solid #ffffff10;
}
.gpu-line {
  display: flex;
  justify-content: space-between;
  align-items: center;
  font-size: 9px;
}
.gpu-name {
  color: #8a93a8;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.temp {
  font-variant-numeric: tabular-nums;
  flex-shrink: 0;
}
.gpu-none {
  font-size: 9px;
  color: #5b6478;
  margin-top: 2px;
}
</style>
