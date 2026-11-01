/**
 * 浏览器预览 Mock：在非 Tauri 环境（直接用浏览器打开 vite dev server）时，
 * 模拟 __TAURI_INTERNALS__ 的 invoke / transformCallback / metadata，
 * 并注入示例数据，用于预览和调整 UI 样式。
 * 真实 Tauri 环境不加载本文件的任何逻辑。
 */
const w = window as unknown as Record<string, unknown>;

if (!w.__TAURI_INTERNALS__) {
  let callbackId = 0;
  const eventHandlers = new Map<string, number[]>(); // event -> handler ids

  const now = Date.now();
  const mockConfig = {
    servers: [
      { id: "srv-gpu", name: "gpu-server-01", url: "192.168.1.10:9101", token: null },
      { id: "srv-cpu", name: "cpu-build-02", url: "192.168.1.21:9101", token: null },
    ],
    glm: { base_url: "https://open.bigmodel.cn", api_key: null },
    gpt: { access_token: "mock", account_id: null },
    update_manifest_url: null,
    update_token: null,
    refresh_glm_secs: 60,
    refresh_server_secs: 3,
    opacity: 1.0,
    collapsed: false,
    window_pos: null,
  };

  function mockGptUpdate() {
    return {
      ok: true,
      error: null,
      configured: true,
      key_source: "manual",
      plan: "Pro",
      five_hour: {
        used_percent: 34,
        reset_at_ms: now + 1.8 * 3600 * 1000,
        window_secs: 18000,
      },
      weekly: {
        used_percent: 72,
        reset_at_ms: now + 3.1 * 24 * 3600 * 1000,
        window_secs: 604800,
      },
      credits_balance: null,
      fetched_at_ms: Date.now(),
    };
  }

  const jitter = (base: number, amp: number) =>
    Math.max(0, Math.min(100, base + (Math.random() - 0.5) * 2 * amp));

  function mockGlmUpdate() {
    return {
      ok: true,
      error: null,
      level: "max",
      five_hour: {
        percentage: 23,
        next_reset_ms: now + 2.2 * 3600 * 1000,
        usage: 2760,
        current: null,
        remaining: null,
        details: {},
      },
      weekly: {
        percentage: 58,
        next_reset_ms: now + 4.3 * 24 * 3600 * 1000,
        usage: 34800,
        current: null,
        remaining: null,
        details: {},
      },
      mcp: {
        percentage: 100,
        next_reset_ms: now + 1.2 * 24 * 3600 * 1000,
        usage: 4000,
        current: 4000,
        remaining: 0,
        details: { "search-prime": 3955, "web-reader": 45 },
      },
      key_source: "zcode",
      fetched_at_ms: Date.now(),
    };
  }

  function mockServerUpdate(id: string) {
    if (id === "srv-gpu") {
      return {
        id,
        online: true,
        error: null,
        latency_ms: 12,
        metrics: {
          hostname: "gpu-server-01",
          cpu_percent: Math.round(jitter(32, 6) * 10) / 10,
          cpu_cores: [],
          mem_used_gb: 51.2,
          mem_total_gb: 128,
          gpus: [
            {
              index: 0,
              name: "NVIDIA GeForce RTX 4090",
              util: Math.round(jitter(58, 8)),
              mem_used_mb: 8397,
              mem_total_mb: 24564,
              temp_c: 67,
            },
            {
              index: 1,
              name: "NVIDIA A100-SXM4-80GB",
              util: Math.round(jitter(96, 3)),
              mem_used_mb: 72154,
              mem_total_mb: 81920,
              temp_c: 88,
            },
          ],
          agent_ts_ms: Date.now(),
        },
      };
    }
    return { id, online: false, error: "连接失败: connection refused", metrics: null, latency_ms: null };
  }

  function dispatch(event: string, payload: unknown) {
    for (const handlerId of eventHandlers.get(event) ?? []) {
      const fn = w[`_${handlerId}`] as ((e: unknown) => void) | undefined;
      fn?.({ event, id: handlerId, payload });
    }
  }

  w.__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main" } },
    transformCallback(cb: (e: unknown) => void) {
      const id = ++callbackId;
      Object.defineProperty(w, `_${id}`, { value: cb, configurable: true });
      return id;
    },
    async invoke(cmd: string, args: Record<string, unknown> = {}) {
      switch (cmd) {
        case "get_config":
          return mockConfig;
        case "plugin:event|listen": {
          const { event, handler } = args as { event: string; handler: number };
          const list = eventHandlers.get(event) ?? [];
          list.push(handler);
          eventHandlers.set(event, list);
          return handler;
        }
        case "plugin:event|unlisten":
          return null;
        case "plugin:app|version":
          return "0.2.8-preview";
        case "plugin:autostart|is_enabled":
          return false;
        case "plugin:window|hide":
          console.log("[mock] hide window");
          return null;
        case "set_collapsed":
          mockConfig.collapsed = args.collapsed as boolean;
          applyPreviewSize();
          return null;
        case "resize_widget": {
          const { width, height } = args as { width: number; height: number };
          document.body.style.width = `${Math.round(width)}px`;
          document.body.style.height = `${Math.round(height)}px`;
          return null;
        }
        case "save_config":
          Object.assign(mockConfig, args.config);
          return null;
        case "test_glm_now":
          return mockGlmUpdate();
        case "test_gpt_now":
          return mockGptUpdate();
        case "detect_codex":
          return {
            found: true,
            path: "~/.codex/auth.json",
            token: "mock-token",
            accountId: "acc-mock",
            hint: null,
          };
        case "check_update":
          return {
            current_version: "0.1.0",
            available: false,
            latest_version: "0.1.0",
            notes: "初始版本",
            asset_url: null,
            asset_size: null,
            error: null,
          };
        case "perform_update":
          return null;
        default:
          console.log("[mock] invoke", cmd, args);
          return null;
      }
    },
  };

  // 预览背景：模拟深色桌面
  document.documentElement.style.background =
    "radial-gradient(1200px 700px at 70% 20%, #2b3550 0%, #171c28 45%, #0c0f16 100%)";
  document.documentElement.style.height = "100%";
  document.documentElement.style.display = "flex";
  document.documentElement.style.alignItems = "flex-start";
  document.documentElement.style.padding = "24px";
  (document.body.style as CSSStyleDeclaration).width = "340px";
  // 模拟 Rust 端 apply_window_size：收起固定两行 400x62，展开 340x560
  const applyPreviewSize = () => {
    document.body.style.width = mockConfig.collapsed ? "400px" : "340px";
    document.body.style.height = mockConfig.collapsed ? "62px" : "560px";
  };
  applyPreviewSize();

  function mockTiboUpdate() {
    return {
      ok: true,
      error: null,
      preview_note: null,
      next_date: "11-02",
      next_date_full: "2026-11-02",
      summary: "Day 26/ Next two days will be resets… banked… do with this what you may",
      url: "https://x.com/thsottiaux/status/2116405414921384099",
      announced_at: new Date().toISOString(),
      confidence: "medium",
      signal_summary: "Lands tomorrow before 10am PST.",
      stale: false,
      fetched_at_ms: Date.now(),
    };
  }

  // 注入示例数据流
  setTimeout(() => {
    dispatch("glm-update", mockGlmUpdate());
    dispatch("gpt-update", mockGptUpdate());
    dispatch("tibo-update", mockTiboUpdate());
    for (const s of mockConfig.servers) dispatch("server-update", mockServerUpdate(s.id));
  }, 300);
  setInterval(() => {
    for (const s of mockConfig.servers) dispatch("server-update", mockServerUpdate(s.id));
  }, 2000);

  console.log("[monitor-plus] browser preview mock active");
}
