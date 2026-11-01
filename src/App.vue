<script setup lang="ts">
import { ref, onMounted, onUnmounted, computed, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import GlmPanel from "./components/GlmPanel.vue";
import GptPanel from "./components/GptPanel.vue";
import ServerCard from "./components/ServerCard.vue";
import SettingsPanel from "./components/SettingsPanel.vue";
import type { QuotaSnapshot, GptSnapshot, TiboSnapshot, ServerUpdate, AppConfig } from "./types";

const glmSnap = ref<QuotaSnapshot | null>(null);
const gptSnap = ref<GptSnapshot | null>(null);
const tiboSnap = ref<TiboSnapshot | null>(null);
const serverMap = ref<Record<string, ServerUpdate>>({});
const config = ref<AppConfig | null>(null);
const opacity = ref(1);
const collapsed = ref(false);
const showSettings = ref(false);

const serverNames = computed(() => {
  const names: { id: string; name: string }[] = [];
  for (const s of config.value?.servers ?? []) {
    names.push({ id: s.id, name: s.name || s.url });
  }
  return names;
});

function loadColor(pct: number): string {
  if (pct >= 90) return "#f56c6c";
  if (pct >= 70) return "#e6a23c";
  return "#67c23a";
}

/** 收起条悬停提示用：距额度重置还有多久 */
function resetCountdown(ms: number | null | undefined): string {
  if (!ms) return "重置时间未知";
  const diff = ms - Date.now();
  if (diff <= 0) return "即将重置";
  const mins = Math.floor(diff / 60000);
  const h = Math.floor(mins / 60);
  const m = mins % 60;
  if (h >= 24) return `${Math.floor(h / 24)}天${h % 24}时后重置`;
  return h > 0 ? `${h}时${m}分后重置` : `${m}分后重置`;
}

let unlisteners: UnlistenFn[] = [];


onMounted(async () => {
  config.value = await invoke<AppConfig>("get_config");
  opacity.value = config.value.opacity;
  collapsed.value = config.value.collapsed;

  unlisteners.push(
    await listen<QuotaSnapshot>("glm-update", (e) => (glmSnap.value = e.payload)),
    await listen<GptSnapshot>("gpt-update", (e) => (gptSnap.value = e.payload)),
    await listen<TiboSnapshot>("tibo-update", (e) => (tiboSnap.value = e.payload)),
    await listen<ServerUpdate>("server-update", (e) => {
      serverMap.value[e.payload.id] = e.payload;
    }),
    await listen<number>("opacity-changed", (e) => (opacity.value = e.payload)),
  );
});

onUnmounted(() => {
  unlisteners.forEach((u) => u());
  window.clearTimeout(adaptiveTimer);
});

function toggleCollapse() {
  collapsed.value = !collapsed.value;
  invoke("set_collapsed", { collapsed: collapsed.value });
}

function hideWindow() {
  getCurrentWindow().hide().catch((e) => console.error("hide failed:", e));
}

/** Tibo 重置官宣：收起条展示剩余天数/小时数（不显示日期），悬停含日期与原话 */
const tiboMsLeft = computed(() => {
  const full = tiboSnap.value?.next_date_full;
  if (!tiboSnap.value?.ok || !full) return null;
  const target = Date.parse(full + "T23:59:59Z"); // 该 UTC 日期结束（近似 PT 当天结束）
  return target - Date.now();
});
const tiboNextShort = computed(() => {
  // 预告回退：无确定日期但 Tibo 预告了近期重置
  if (tiboSnap.value?.preview_note) return "预告 1-2 天内";
  const ms = tiboMsLeft.value;
  if (ms == null) return "无计划";
  if (ms <= 0) return "进行中";
  const h = Math.floor(ms / 3600000);
  if (h >= 48) return `还有 ${Math.floor(h / 24)} 天`;
  if (h >= 1) return `还有 ${h} 小时`;
  return `还有 ${Math.max(1, Math.round(ms / 60000))} 分钟`;
});
const tiboColor = computed(() => {
  if (tiboSnap.value?.preview_note) return "#e6a23c"; // 预告 = 橙
  const ms = tiboMsLeft.value ?? Infinity;
  if (ms <= 24 * 3600000) return "#f56c6c";
  if (ms <= 72 * 3600000) return "#e6a23c";
  return "#67c23a";
});
const tiboTitle = computed(() => {
  const t = tiboSnap.value;
  if (!t?.ok) return t?.error ?? "Tibo 重置官宣获取中…";
  if (t.preview_note) return `${t.preview_note}\n${t.summary ?? ""}${t.url ? "\n" + t.url : ""}`;
  if (!t.next_date_full) return "Tibo 尚无下一次重置的官宣计划";
  const ms = tiboMsLeft.value ?? 0;
  const h = Math.max(1, Math.ceil(ms / 3600000));
  const when = ms <= 0 ? "已开始" : h >= 48 ? `约 ${Math.floor(h / 24)} 天（${h} 小时）` : `约 ${h} 小时`;
  return `Tibo 下次重置还有 ${when}（${t.next_date_full}）`;
});

/** 收起模式第二行：服务器小图标条 */
const miniServers = computed(() =>
  serverNames.value.map((s) => {
    const u = serverMap.value[s.id];
    const m = u?.metrics;
    const online = !!u?.online && !!m;
    const cpu = m ? Math.round(m.cpu_percent) : null;
    const gpu =
      m && m.gpus.length > 0 ? Math.round(Math.max(...m.gpus.map((g) => g.util))) : null;
    const name = s.name.length > 8 ? s.name.slice(0, 7) + "…" : s.name;
    return {
      id: s.id,
      name,
      online,
      cpu,
      gpu,
      title: online
        ? `${s.name} · CPU ${cpu}%${gpu != null ? ` · GPU 峰值 ${gpu}%` : ""}`
        : `${s.name}: 离线`,
    };
  }),
);

function onSaved(cfg: AppConfig) {
  config.value = cfg;
  // 配置里删掉的服务器，清掉本地缓存
  const ids = new Set(cfg.servers.map((s) => s.id));
  for (const k of Object.keys(serverMap.value)) {
    if (!ids.has(k)) delete serverMap.value[k];
  }
  // 收起状态下服务器数量变化后重新适配窗口尺寸
  if (collapsed.value) invoke("set_collapsed", { collapsed: true });
}
/** 收起态宽度自适应：量出 .mini 内容实际宽度，通知 Rust 调整窗口 */
let adaptiveTimer: number | undefined;
function requestAdaptiveWidth() {
  // 注意不能拿 showSettings 当守卫：展开态开着设置面板再收起时，
  // showSettings 仍为 true，会把收起条永久卡在固定宽度（v0.2.2 卡 400 的根因）。
  // 设置面板只渲染在展开分支里，收起态它必然已卸载，无需防它。
  if (!collapsed.value) return;
  window.clearTimeout(adaptiveTimer);
  adaptiveTimer = window.setTimeout(() => {
    const mini = document.querySelector(".mini") as HTMLElement | null;
    if (!mini) return;
    const prev = mini.style.width;
    mini.style.width = "max-content";
    const content = Math.ceil(mini.getBoundingClientRect().width);
    mini.style.width = prev;
    const hasServers = !!document.querySelector(".mini-row.srvs");
    const target = Math.min(900, Math.max(300, content + 2)); // +2 补窗口边框
    // 与当前窗口实际宽度比，而不是上次请求值：折叠状态切换后
    // lastRequestedW 已过期，会导致该调的 resize 被阈值吞掉
    if (Math.abs(target - window.innerWidth) < 3) return; // 已在目标宽度，不抖动
    invoke("resize_widget", { width: target, height: hasServers ? 62 : 36 });
  }, 200);
}

watch([collapsed, glmSnap, gptSnap, miniServers, serverNames], requestAdaptiveWidth);
watch(showSettings, requestAdaptiveWidth);

</script>

<template>
  <div class="widget" :style="{ opacity }">
    <!-- 折叠：两行紧凑条（上：大模型，下：服务器） -->
    <div v-if="collapsed" class="mini" @dblclick="toggleCollapse">
      <div class="mini-row" data-tauri-drag-region>
        <span class="mini-brand" data-tauri-drag-region>GLM</span>
        <span class="mv" :title="`5小时额度 ${resetCountdown(glmSnap?.five_hour?.next_reset_ms)}`">
          5h <b :style="{ color: glmSnap?.five_hour ? loadColor(glmSnap.five_hour.percentage) : '#f56c6c' }">
            {{ glmSnap?.five_hour?.percentage ?? "--" }}%</b>
        </span>
        <span class="mv" :title="`每周额度 ${resetCountdown(glmSnap?.weekly?.next_reset_ms)}`">
          周 <b :style="{ color: glmSnap?.weekly ? loadColor(glmSnap.weekly.percentage) : '#f56c6c' }">
            {{ glmSnap?.weekly?.percentage ?? "--" }}%</b>
        </span>
        <i v-if="gptSnap?.configured" class="mini-sep" />
        <span
          v-if="gptSnap?.configured"
          class="mini-brand gpt"
          :title="gptSnap?.plan ? `GPT Codex ${gptSnap.plan} 套餐` : 'GPT Codex'"
        >
          GPT
        </span>
        <template v-if="gptSnap?.configured">
          <span class="mv" :title="`GPT 5小时额度 ${resetCountdown(gptSnap?.five_hour?.reset_at_ms)}`">
            5h <b :style="{ color: gptSnap?.five_hour ? loadColor(gptSnap.five_hour.used_percent) : '#f56c6c' }">
              {{ gptSnap?.five_hour?.used_percent ?? "--" }}%</b>
          </span>
          <span class="mv" :title="`GPT 每周额度 ${resetCountdown(gptSnap?.weekly?.reset_at_ms)}`">
            周 <b :style="{ color: gptSnap?.weekly ? loadColor(gptSnap.weekly.used_percent) : '#f56c6c' }">
              {{ gptSnap?.weekly?.used_percent ?? "--" }}%</b>
          </span>
          <i class="mini-sep" />
          <span class="mv" :title="tiboTitle">
            重置 <b :style="{ color: tiboColor }">{{ tiboNextShort }}</b>
          </span>
        </template>
        <button class="mini-expand" title="展开" @click="toggleCollapse">⌃</button>
      </div>
      <div v-if="miniServers.length" class="mini-row srvs">
        <span v-for="s in miniServers" :key="s.id" class="mini-srv" :title="s.title">
          <i class="sdot" :style="{ background: s.online ? '#67c23a' : '#f56c6c' }" />
          <span class="sname">{{ s.name }}</span>
          <span v-if="s.online" class="svals">
            <b :style="{ color: loadColor(s.cpu!) }">{{ s.cpu }}%</b>
            <i class="sep">·</i>
            <b v-if="s.gpu != null" :style="{ color: loadColor(s.gpu) }">{{ s.gpu }}%</b>
            <b v-else class="nogpu">—</b>
          </span>
          <span v-else class="svals off">离线</span>
        </span>
      </div>
    </div>

    <!-- 展开：完整面板 -->
    <div v-else class="full">
      <header class="titlebar" data-tauri-drag-region>
        <span class="brand" data-tauri-drag-region>Monitor<span class="brand-plus">+</span></span>
        <span class="title-actions">
          <button class="tb-btn" title="折叠" @click="toggleCollapse">⌄</button>
          <button class="tb-btn" title="设置" @click="showSettings = !showSettings">⚙</button>
          <button class="tb-btn close" title="隐藏到托盘" @click="hideWindow">✕</button>
        </span>
      </header>

      <SettingsPanel
        v-if="showSettings && config"
        :config="config"
        @close="showSettings = false"
        @saved="onSaved"
      />

      <template v-else-if="config">
        <div class="content">
          <GlmPanel :snap="glmSnap" />
          <GptPanel :snap="gptSnap" :tibo="tiboSnap" />
          <div class="section-head">服务器</div>
          <div class="server-list">
            <ServerCard
              v-for="s in serverNames"
              :key="s.id"
              :name="s.name"
              :update="serverMap[s.id] ?? null"
            />
            <div v-if="config.servers.length === 0" class="no-server">
              尚未添加服务器，点击 ⚙ 添加
            </div>
          </div>
        </div>
      </template>
    </div>
  </div>
</template>

<style>
* {
  margin: 0;
  padding: 0;
  box-sizing: border-box;
}
html, body, #app {
  height: 100%;
  background: transparent;
  overflow: hidden;
  user-select: none;
}
body {
  font-family: -apple-system, "Segoe UI", "Microsoft YaHei", sans-serif;
}
.widget {
  height: 100%;
  display: flex;
  flex-direction: column;
  background: linear-gradient(168deg, #151a23f2 0%, #10141cf2 100%);
  border: 1px solid #ffffff14;
  border-radius: 12px;
  overflow: hidden;
  box-shadow: 0 6px 24px #00000066;
}
.full {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
}
.titlebar {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 8px 8px 6px 12px;
  flex-shrink: 0;
}
.brand {
  font-size: 12px;
  font-weight: 800;
  color: #e8ecf4;
  letter-spacing: 0.02em;
}
.brand-plus {
  color: #6b6bff;
}
.title-actions {
  display: flex;
  gap: 2px;
}
.tb-btn {
  background: none;
  border: none;
  color: #6b7488;
  width: 24px;
  height: 20px;
  border-radius: 5px;
  cursor: pointer;
  font-size: 11px;
  line-height: 1;
}
.tb-btn:hover {
  background: #ffffff14;
  color: #d0d6e4;
}
.tb-btn.close:hover {
  background: #f56c6c33;
  color: #f56c6c;
}
.no-server {
  font-size: 10px;
  color: #5b6478;
  text-align: center;
  padding: 14px 0;
}
.content {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding-bottom: 6px;
}
.section-head {
  font-size: 9px;
  font-weight: 700;
  color: #6b7488;
  text-transform: uppercase;
  letter-spacing: 0.08em;
  padding: 8px 12px 2px;
}
.server-list {
  display: flex;
  flex-direction: column;
  gap: 7px;
  padding: 5px 10px 4px;
}

/* 折叠：两行紧凑条（上大模型 / 下服务器） */
.mini {
  height: 100%;
  display: flex;
  flex-direction: column;
  justify-content: center;
  gap: 3px;
  padding: 5px 8px 5px 10px;
}
.mini-row {
  display: flex;
  align-items: center;
  gap: 5px;
  min-width: 0;
  min-height: 0;
}
.mini-row.srvs {
  gap: 12px;
}
.mini-brand {
  font-size: 10px;
  font-weight: 800;
  color: #e8ecf4;
  flex-shrink: 0;
}
.mini-brand.gpt {
  color: #b9b9ff;
}
.mv {
  font-size: 10px;
  color: #8a93a8;
  white-space: nowrap;
  flex-shrink: 0;
  font-variant-numeric: tabular-nums;
}
.mv b {
  font-weight: 700;
}
.mv.hot b {
  color: #e6a23c;
}
.mini-sep {
  width: 1px;
  height: 13px;
  background: #ffffff1c;
  flex-shrink: 0;
}
.mini-srv {
  display: flex;
  align-items: center;
  gap: 4px;
  min-width: 0;
  flex-shrink: 1;
  overflow: hidden;
}
.sdot {
  width: 5px;
  height: 5px;
  border-radius: 50%;
  flex-shrink: 0;
}
.sname {
  font-size: 9.5px;
  font-weight: 600;
  color: #d0d6e4;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  max-width: 88px;
}
.svals {
  flex-shrink: 0;
  font-size: 9.5px;
  font-variant-numeric: tabular-nums;
}
.svals b {
  font-weight: 600;
}
.svals .sep {
  font-style: normal;
  color: #3d4356;
  margin: 0 2px;
}
.svals.off {
  color: #f56c6caa;
  font-size: 9px;
}
.svals .nogpu {
  color: #3d4356;
}
.mini-expand {
  margin-left: auto;
  background: none;
  border: none;
  color: #6b7488;
  cursor: pointer;
  font-size: 12px;
  padding: 2px 6px;
  border-radius: 4px;
  flex-shrink: 0;
}
.mini-expand:hover {
  background: #ffffff14;
  color: #d0d6e4;
}

/* 滚动条 */
::-webkit-scrollbar {
  width: 5px;
}
::-webkit-scrollbar-thumb {
  background: #ffffff1a;
  border-radius: 3px;
}
</style>
