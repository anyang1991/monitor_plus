export interface LimitEntry {
  percentage: number;
  next_reset_ms: number | null;
  usage: number | null;
  current: number | null;
  remaining: number | null;
  details: Record<string, number>;
}

export interface QuotaSnapshot {
  ok: boolean;
  error: string | null;
  level: string | null;
  five_hour: LimitEntry | null;
  weekly: LimitEntry | null;
  mcp: LimitEntry | null;
  key_source: string | null;
  fetched_at_ms: number;
}

export interface GpuInfo {
  index: number;
  name: string;
  util: number;
  mem_used_mb: number;
  mem_total_mb: number;
  temp_c: number | null;
}

export interface AgentMetrics {
  hostname: string;
  cpu_percent: number;
  cpu_cores: number[];
  mem_used_gb: number | null;
  mem_total_gb: number | null;
  gpus: GpuInfo[];
  agent_ts_ms: number;
}

export interface ServerUpdate {
  id: string;
  online: boolean;
  error: string | null;
  metrics: AgentMetrics | null;
  latency_ms: number | null;
}

export interface ServerConfig {
  id: string;
  name: string;
  url: string;
  token: string | null;
}

export interface GlmConfig {
  base_url: string;
  api_key: string | null;
}

export interface GptWindowUsage {
  used_percent: number;
  reset_at_ms: number | null;
  window_secs: number;
}

export interface GptSnapshot {
  ok: boolean;
  error: string | null;
  configured: boolean;
  key_source: string | null;
  plan: string | null;
  plan_debug: string | null;
  five_hour: GptWindowUsage | null;
  weekly: GptWindowUsage | null;
  credits_balance: string | null;
  fetched_at_ms: number;
}

export interface GptConfig {
  access_token: string | null;
  account_id: string | null;
}

export interface CodexDetect {
  found: boolean;
  path: string | null;
  token: string | null;
  accountId: string | null;
  hint: string | null;
}

export interface TiboSnapshot {
  ok: boolean;
  error: string | null;
  next_date: string | null;
  next_date_full: string | null;
  summary: string | null;
  url: string | null;
  announced_at: string | null;
  confidence: string | null;
  preview_note: string | null;
  signal_summary: string | null;
  stale: boolean;
  fetched_at_ms: number;
}

export interface UpdateCheck {
  current_version: string;
  available: boolean;
  latest_version: string | null;
  notes: string | null;
  asset_url: string | null;
  asset_size: number | null;
  error: string | null;
}

export interface AppConfig {
  servers: ServerConfig[];
  glm: GlmConfig;
  gpt: GptConfig;
  update_manifest_url: string | null;
  update_token: string | null;
  refresh_glm_secs: number;
  refresh_server_secs: number;
  opacity: number;
  collapsed: boolean;
  window_pos: [number, number] | null;
}
