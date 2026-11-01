<script setup lang="ts">
import { reactive, ref, onMounted, onUnmounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { enable, disable, isEnabled } from "@tauri-apps/plugin-autostart";
import type { AppConfig, ServerConfig, QuotaSnapshot, GptSnapshot, CodexDetect, UpdateCheck, AgentMetrics } from "../types";

const props = defineProps<{ config: AppConfig }>();
const emit = defineEmits<{ close: []; saved: [AppConfig] }>();

const draft = reactive<AppConfig>(JSON.parse(JSON.stringify(props.config)));
const draftKey = ref(draft.glm.api_key ?? "");
const draftUpdateUrl = ref(draft.update_manifest_url ?? "");
const draftUpdateToken = ref(draft.update_token ?? "");
const detect = ref<{ loading: boolean; msg: string; ok: boolean }>({ loading: false, msg: "", ok: false });

// 更新状态
const upd = ref<{
  checking: boolean;
  installing: boolean;
  current: string;
  available: boolean;
  latest: string;
  notes: string;
  url: string;
  percent: number | null;
  msg: string;
  msgOk: boolean;
}>({
  checking: false,
  installing: false,
  current: "…",
  available: false,
  latest: "",
  notes: "",
  url: "",
  percent: null,
  msg: "",
  msgOk: false,
});
let unlistenProgress: UnlistenFn | null = null;

// 测试状态
const glmTest = ref<{ loading: boolean; msg: string; ok: boolean }>({ loading: false, msg: "", ok: false });
const gptTest = ref<{ loading: boolean; msg: string; ok: boolean }>({ loading: false, msg: "", ok: false });
const srvTest = ref<Record<string, { loading: boolean; msg: string; ok: boolean }>>({});

const autoStart = ref(false);
onMounted(async () => {
  try {
    upd.value.current = await getVersion();
  } catch {
    upd.value.current = "?";
  }
  try {
    autoStart.value = await isEnabled();
  } catch {
    autoStart.value = false;
  }
  unlistenProgress = await listen<{ percent: number | null }>("update-progress", (e) => {
    upd.value.percent = e.payload.percent;
  });
});
onUnmounted(() => unlistenProgress?.());

async function checkUpdate() {
  upd.value.checking = true;
  upd.value.msg = "";
  try {
    const r = await invoke<UpdateCheck>("check_update");
    upd.value.current = r.current_version;
    if (r.error) {
      upd.value.msg = r.error;
      upd.value.msgOk = false;
    } else if (r.available && r.asset_url) {
      upd.value.available = true;
      upd.value.latest = r.latest_version ?? "";
      upd.value.notes = r.notes ?? "";
      upd.value.url = r.asset_url;
      upd.value.msg = "";
    } else if (r.latest_version && !r.available) {
      upd.value.msg = `已是最新版本（v${r.current_version}）${r.notes ? " · " + r.notes : ""}`;
      upd.value.msgOk = true;
    } else {
      upd.value.msg = r.notes ?? "未知状态";
      upd.value.msgOk = false;
    }
  } catch (e) {
    upd.value.msg = String(e);
    upd.value.msgOk = false;
  } finally {
    upd.value.checking = false;
  }
}

async function doUpdate() {
  if (!upd.value.url) return;
  upd.value.installing = true;
  upd.value.percent = 0;
  upd.value.msg = "下载中…";
  try {
    await invoke("perform_update", { url: upd.value.url });
    // 成功时应用会自动退出并运行安装器
  } catch (e) {
    upd.value.installing = false;
    upd.value.percent = null;
    upd.value.msg = String(e);
    upd.value.msgOk = false;
  }
}

async function toggleAutoStart() {
  try {
    if (autoStart.value) {
      await disable();
      autoStart.value = false;
    } else {
      await enable();
      autoStart.value = true;
    }
  } catch (e) {
    console.error("autostart failed", e);
  }
}

function addServer() {
  const id = `srv-${Date.now()}`;
  draft.servers.push({ id, name: "", url: "", token: null });
  srvTest.value[id] = { loading: false, msg: "", ok: false };
}

function removeServer(idx: number) {
  const id = draft.servers[idx].id;
  draft.servers.splice(idx, 1);
  delete srvTest.value[id];
}

async function testServer(s: ServerConfig) {
  srvTest.value[s.id] = { loading: true, msg: "测试中…", ok: false };
  try {
    const m = await invoke<AgentMetrics>("test_server", {
      server: { id: s.id, name: s.name, url: s.url, token: s.token },
    });
    const gpu = m.gpus.length > 0 ? ` · GPU×${m.gpus.length}` : "";
    srvTest.value[s.id] = {
      loading: false,
      ok: true,
      msg: `${m.hostname} CPU ${m.cpu_percent}%${gpu}`,
    };
  } catch (e) {
    srvTest.value[s.id] = { loading: false, ok: false, msg: String(e) };
  }
}

async function testGlm() {
  glmTest.value = { loading: true, msg: "测试中…", ok: false };
  const payload = JSON.parse(JSON.stringify(draft)) as AppConfig;
  payload.glm.api_key = draftKey.value.trim() || null;
  try {
    const snap = await invoke<QuotaSnapshot>("test_glm_now", { config: payload });
    if (snap.ok) {
      glmTest.value = {
        loading: false,
        ok: true,
        msg: `成功 · 5h ${snap.five_hour?.percentage ?? "?"}% / 周 ${snap.weekly?.percentage ?? "?"}%`,
      };
    } else {
      glmTest.value = { loading: false, ok: false, msg: snap.error ?? "失败" };
    }
  } catch (e) {
    glmTest.value = { loading: false, ok: false, msg: String(e) };
  }
}

async function testGpt() {
  gptTest.value = { loading: true, msg: "测试中…", ok: false };
  // draft.gpt 已由自动检测（或之前保存的配置）填充
  const payload = JSON.parse(JSON.stringify(draft)) as AppConfig;
  try {
    const snap = await invoke<GptSnapshot>("test_gpt_now", { config: payload });
    if (snap.ok) {
      gptTest.value = {
        loading: false,
        ok: true,
        msg: `成功 · 5h ${snap.five_hour?.used_percent ?? "?"}% / 周 ${snap.weekly?.used_percent ?? "?"}%`,
      };
    } else {
      gptTest.value = { loading: false, ok: false, msg: snap.error ?? "失败" };
    }
  } catch (e) {
    gptTest.value = { loading: false, ok: false, msg: String(e) };
  }
}

async function detectCodex() {
  detect.value = { loading: true, msg: "检测中…", ok: false };
  try {
    const r = await invoke<CodexDetect>("detect_codex");
    if (r.found && r.token) {
      // 写入草稿，保存/测试都会带上
      draft.gpt.access_token = r.token;
      draft.gpt.account_id = r.accountId ?? null;
      detect.value = { loading: false, ok: true, msg: `已读取 ${r.path}` };
      await testGpt();
    } else {
      detect.value = { loading: false, ok: false, msg: r.hint ?? "未找到 Codex 凭据" };
    }
  } catch (e) {
    detect.value = { loading: false, ok: false, msg: String(e) };
  }
}

const saveErr = ref("");
async function save() {
  saveErr.value = "";
  draft.glm.api_key = draftKey.value.trim() || null;
  draft.update_manifest_url = draftUpdateUrl.value.trim() || null;
  draft.update_token = draftUpdateToken.value.trim() || null;
  try {
    await invoke("save_config", { config: JSON.parse(JSON.stringify(draft)) });
  } catch (e) {
    // 保存失败必须可见：不再静默吞掉导致"填了白填"
    saveErr.value = String(e);
    return;
  }
  emit("saved", JSON.parse(JSON.stringify(draft)));
  emit("close");
}
</script>

<template>
  <div class="settings">
    <div class="set-head">
      <span>设置</span>
      <span class="head-actions">
        <span
          v-if="saveErr"
          class="head-err"
          :title="saveErr"
        >⚠</span>
        <button
          class="mini-btn primary head-save"
          :class="{ 'save-failed': !!saveErr }"
          :title="saveErr || '保存并关闭'"
          @click="save"
        >{{ saveErr ? "保存失败" : "保存" }}</button>
        <button class="icon-btn" title="不保存关闭" @click="emit('close')">✕</button>
      </span>
    </div>

    <div class="set-body">
      <!-- GLM -->
      <div class="section-title">GLM Coding Plan</div>
      <div class="field-row">
        <label>接口端点</label>
        <select v-model="draft.glm.base_url">
          <option value="https://open.bigmodel.cn">bigmodel.cn（中国）</option>
          <option value="https://api.z.ai">api.z.ai（全球）</option>
        </select>
      </div>
      <div class="field-row">
        <label>API Key</label>
        <input
          v-model="draftKey"
          type="password"
          placeholder="留空自动读取本机 ZCode 登录凭据"
          spellcheck="false"
        />
      </div>
      <div class="test-row">
        <button class="mini-btn" :disabled="glmTest.loading" @click="testGlm">测试</button>
        <span class="test-msg" :class="glmTest.ok ? 'ok' : 'err'" :title="glmTest.msg">{{ glmTest.msg }}</span>
      </div>
      <div class="field-row">
        <label>刷新间隔</label>
        <select v-model.number="draft.refresh_glm_secs">
          <option :value="60">60 秒</option>
          <option :value="120">2 分钟</option>
          <option :value="300">5 分钟</option>
        </select>
      </div>

      <!-- GPT (Codex 订阅) -->
      <div class="section-title">GPT Codex 订阅</div>
      <div class="field-row">
        <button class="mini-btn primary wide" :disabled="detect.loading" @click="detectCodex">
          {{ detect.loading ? "检测中…" : "🔍 自动检测 Codex 凭据" }}
        </button>
      </div>
      <div v-if="detect.msg" class="test-row" style="margin-left: 0">
        <span class="test-msg" :class="detect.ok ? 'ok' : 'err'" :title="detect.msg">{{ detect.msg }}</span>
      </div>
      <div class="test-row">
        <button class="mini-btn" :disabled="gptTest.loading" @click="testGpt">测试</button>
        <span class="test-msg" :class="gptTest.ok ? 'ok' : 'err'" :title="gptTest.msg">{{ gptTest.msg }}</span>
      </div>

      <!-- 服务器 -->
      <div class="section-title with-action">
        服务器
        <button class="mini-btn accent" @click="addServer">＋ 添加</button>
      </div>
      <div v-for="(s, i) in draft.servers" :key="s.id" class="server-edit">
        <div class="server-row">
          <input v-model="s.name" placeholder="名称" class="w-name" spellcheck="false" />
          <input v-model="s.url" placeholder="IP 或 http://IP:9101" class="w-url" spellcheck="false" />
          <button class="icon-btn danger" title="删除" @click="removeServer(i)">🗑</button>
        </div>
        <div class="server-row">
          <input v-model="s.token" placeholder="访问令牌（可选）" type="password" class="w-token" spellcheck="false" />
          <button class="mini-btn" :disabled="!s.url || srvTest[s.id]?.loading" @click="testServer(s)">测试</button>
        </div>
        <div v-if="srvTest[s.id]?.msg" class="test-row">
          <span class="test-msg" :class="srvTest[s.id].ok ? 'ok' : 'err'">{{ srvTest[s.id].msg }}</span>
        </div>
      </div>
      <div v-if="draft.servers.length === 0" class="empty-hint">尚未添加服务器</div>
      <div class="field-row">
        <label>刷新间隔</label>
        <select v-model.number="draft.refresh_server_secs">
          <option :value="2">2 秒</option>
          <option :value="3">3 秒</option>
          <option :value="5">5 秒</option>
          <option :value="10">10 秒</option>
        </select>
      </div>

      <!-- 外观 -->
      <div class="section-title">外观与系统</div>
      <div class="field-row">
        <label>窗口不透明度</label>
        <input v-model.number="draft.opacity" type="range" min="0.4" max="1" step="0.05" class="slider" />
        <span class="range-val">{{ Math.round(draft.opacity * 100) }}%</span>
      </div>
      <div class="field-row">
        <label>开机自启</label>
        <button class="mini-btn toggle" :class="{ on: autoStart }" @click="toggleAutoStart">
          {{ autoStart ? "已开启" : "已关闭" }}
        </button>
      </div>

      <!-- 应用更新 -->
      <div class="section-title">应用更新</div>
      <div class="field-row">
        <label>当前版本</label>
        <span class="ver">v{{ upd.current }}</span>
        <button class="mini-btn primary" :disabled="upd.checking || upd.installing" @click="checkUpdate">
          {{ upd.checking ? "检查中…" : "检查更新" }}
        </button>
      </div>
      <div v-if="upd.available" class="update-avail">
        <div class="update-line">
          <b>新版本 v{{ upd.latest }}</b>
          <button class="mini-btn primary" :disabled="upd.installing" @click="doUpdate">
            {{ upd.installing ? (upd.percent != null ? `下载中 ${upd.percent}%` : "准备下载…") : "下载并安装" }}
          </button>
        </div>
        <div v-if="upd.notes" class="update-notes">{{ upd.notes }}</div>
        <div v-if="upd.installing && upd.percent != null" class="bar slim">
          <div class="bar-fill" :style="{ width: upd.percent + '%', background: '#6b6bff' }" />
        </div>
        <div class="update-hint">点击后将自动退出应用并静默安装新版本</div>
      </div>
      <div v-if="upd.msg" class="test-row" style="margin-left: 0">
        <span class="test-msg" :class="upd.msgOk ? 'ok' : 'err'" :title="upd.msg">{{ upd.msg }}</span>
      </div>
      <div class="field-row">
        <label>清单地址</label>
        <input
          v-model="draftUpdateUrl"
          type="text"
          placeholder="留空 = GitHub Releases 默认；自建 GitLab 填 latest.json 的 API 地址"
          spellcheck="false"
        />
      </div>
      <div class="field-row">
        <label>访问令牌</label>
        <input
          v-model="draftUpdateToken"
          type="password"
          placeholder="私有通道需要；公开仓库留空"
          spellcheck="false"
        />
      </div>
    </div>

    <div class="set-foot">
      <span v-if="saveErr" class="save-err" :title="saveErr">保存失败：{{ saveErr }}</span>
      <button class="mini-btn" @click="emit('close')">取消</button>
      <button class="mini-btn primary" @click="save">保存</button>
    </div>
  </div>
</template>

<style scoped>
.settings {
  display: flex;
  flex-direction: column;
  height: 100%;
  background: #14181f;
}
.set-head {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 8px 10px 8px 12px;
  font-size: 12px;
  font-weight: 700;
  color: #d0d6e4;
  border-bottom: 1px solid #ffffff10;
  flex-shrink: 0;
}
.head-actions {
  display: flex;
  align-items: center;
  gap: 6px;
}
.head-save {
  padding: 3px 10px;
}
.head-save.save-failed {
  background: #f56c6c33;
  border-color: #f56c6c66;
  color: #f56c6c;
}
.head-err {
  cursor: help;
  flex-shrink: 0;
}
.set-body {
  flex: 1 1 auto;
  height: 0; /* 双保险：内容不参与高度计算，强制滚动（WebView2 下 min-height:0 不够） */
  min-height: 0;
  overflow-y: auto;
  padding: 8px 12px;
}
.section-title {
  font-size: 10px;
  font-weight: 700;
  color: #7c86a0;
  text-transform: uppercase;
  letter-spacing: 0.06em;
  margin: 12px 0 6px;
}
.section-title.with-action {
  display: flex;
  justify-content: space-between;
  align-items: center;
}
.field-row {
  display: flex;
  align-items: center;
  gap: 8px;
  margin: 6px 0;
}
.field-row > label {
  width: 84px;
  font-size: 10px;
  color: #8a93a8;
  flex-shrink: 0;
}
input, select {
  background: #0d1015;
  border: 1px solid #ffffff14;
  border-radius: 5px;
  color: #d0d6e4;
  font-size: 11px;
  padding: 5px 7px;
  outline: none;
  min-width: 0;
}
input:focus, select:focus {
  border-color: #6b6bff66;
}
.field-row input, .field-row select {
  flex: 1;
}
.field-row input[type="range"] {
  flex: 1;
  padding: 0;
  accent-color: #6b6bff;
}
.range-val {
  width: 34px;
  font-size: 10px;
  color: #8a93a8;
  text-align: right;
  font-variant-numeric: tabular-nums;
}
.server-edit {
  border: 1px solid #ffffff10;
  border-radius: 6px;
  padding: 6px;
  margin: 6px 0;
  background: #ffffff05;
}
.server-row {
  display: flex;
  gap: 5px;
  margin: 4px 0;
}
.w-name { width: 72px; flex-shrink: 0; }
.w-url { flex: 1; }
.w-token { flex: 1; }
.mini-btn {
  background: #ffffff10;
  border: 1px solid #ffffff18;
  color: #b8c0d4;
  font-size: 10px;
  border-radius: 5px;
  padding: 4px 9px;
  cursor: pointer;
  white-space: nowrap;
}
.mini-btn:hover { background: #ffffff1c; }
.mini-btn:disabled { opacity: 0.45; cursor: default; }
.mini-btn.primary {
  background: #5b5bd6;
  border-color: #6b6bff;
  color: #fff;
}
.mini-btn.wide {
  flex: 1;
  padding: 6px 9px;
}
.mini-btn.accent {
  color: #9b9bff;
}
.mini-btn.toggle.on {
  color: #67c23a;
  border-color: #67c23a55;
}
.icon-btn {
  background: none;
  border: none;
  color: #8a93a8;
  cursor: pointer;
  font-size: 12px;
  padding: 2px 5px;
  border-radius: 4px;
}
.icon-btn:hover { background: #ffffff12; }
.icon-btn.danger:hover { color: #f56c6c; }
.test-row {
  display: flex;
  align-items: center;
  gap: 6px;
  margin: 3px 0 1px 90px;
}
.test-msg {
  font-size: 9px;
  /* 版本号+更新说明完整显示：换行而不是截断省略 */
  white-space: normal;
  word-break: break-word;
  line-height: 1.5;
}
.test-msg.ok { color: #67c23a; }
.test-msg.err { color: #f56c6c; }
.empty-hint {
  font-size: 10px;
  color: #5b6478;
  padding: 6px 0;
}
.ver {
  flex: 1;
  font-size: 11px;
  color: #d0d6e4;
  font-variant-numeric: tabular-nums;
}
.update-avail {
  border: 1px solid #6b6bff44;
  background: #6b6bff14;
  border-radius: 6px;
  padding: 7px 9px;
  margin: 5px 0;
}
.update-line {
  display: flex;
  justify-content: space-between;
  align-items: center;
  font-size: 11px;
  color: #d0d6e4;
}
.update-notes {
  font-size: 9px;
  color: #8a93a8;
  margin-top: 4px;
  white-space: pre-wrap;
}
.update-hint {
  font-size: 9px;
  color: #5b6478;
  margin-top: 4px;
}
.save-err {
  flex: 1;
  min-width: 0;
  font-size: 9px;
  color: #f56c6c;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  cursor: help;
}
.set-foot {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  padding: 10px 12px;
  border-top: 1px solid #ffffff10;
}
</style>
