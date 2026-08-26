// @group Types : All API data structures mirroring Rust models

// @group Types > Notifications : Webhook / Slack / Teams notification config
export interface NotificationEvents {
  // Process lifecycle events
  on_crash: boolean
  on_restart: boolean
  on_start: boolean
  on_stop: boolean
  on_unhealthy?: boolean
  on_health_recovered?: boolean
  // Cron job events
  on_cron_run?: boolean
  on_cron_fail?: boolean
}

export interface WebhookTarget {
  url: string
  enabled: boolean
}

export interface SlackTarget {
  webhook_url: string
  enabled: boolean
  channel?: string
}

export interface TeamsTarget {
  webhook_url: string
  enabled: boolean
}

export interface DiscordTarget {
  webhook_url: string
  enabled: boolean
}

export interface NotificationConfig {
  webhook?: WebhookTarget
  slack?: SlackTarget
  teams?: TeamsTarget
  discord?: DiscordTarget
  events: NotificationEvents
  events_override?: boolean
}

export interface NotificationsStore {
  global: NotificationConfig
  namespaces: Record<string, NotificationConfig>
}

export type ProcessStatus =
  | 'stopped'
  | 'starting'
  | 'running'
  | 'stopping'
  | 'crashed'
  | 'errored'
  | 'watching'
  | 'sleeping'

export interface CronRun {
  run_at: string // ISO datetime
  exit_code: number | null
  duration_secs: number
}

export interface ProcessInfo {
  id: string
  project_id: string | null
  name: string
  script: string
  args: string[]
  cwd: string | null
  status: ProcessStatus
  pid: number | null
  restart_count: number
  uptime_secs: number | null
  last_exit_code: number | null
  autorestart: boolean
  max_restarts: number
  watch: boolean
  namespace: string
  created_at: string
  started_at: string | null
  stopped_at: string | null
  cron: string | null
  cron_next_run: string | null
  cron_run_history: CronRun[]
  /** CPU usage percentage (0–100 per core), null when not running */
  cpu_percent: number | null
  /** Resident memory in bytes, null when not running */
  memory_bytes: number | null
  /** Environment variables passed to the process */
  env: Record<string, string>
  /** Process-level notification override */
  notify?: NotificationConfig
  /** Active git branch in the process working directory */
  git_branch?: string
  /** Whether this process participates in bulk Start All operations (default: true) */
  enabled: boolean
}

export interface DaemonHealth {
  status: 'ok' | 'degraded'
  version: string
  uptime_secs: number
  process_count: number
  persistence_healthy: boolean
  persistence_error: string | null
}

export interface LogLine {
  timestamp: string
  stream: 'stdout' | 'stderr'
  content: string
}

// @group Types > Ports : Bounded network-listener entry returned by GET /ports
export interface PortEntry {
  port: number
  protocol: string
  local_address: string
  remote_address: string
  state: string
  pid: number | null
  process_name: string | null
  ancestor_pids?: number[]
}

export interface ScriptInfo {
  name: string
  path: string
  language: string
  size_bytes: number
  modified_at: string
}

export interface StartProcessBody {
  script: string
  name?: string
  project_id?: string
  cwd?: string
  args?: string[]
  env?: Record<string, string>
  namespace?: string
  autorestart?: boolean
  watch?: boolean
  max_restarts?: number
  restart_delay_ms?: number
  watch_paths?: string[]
  cron?: string
  notify?: NotificationConfig
}

export type ProjectKind = 'managed' | 'desktop'
export type ProjectStatus = 'desktop' | 'running' | 'partial' | 'stopped' | 'errored' | 'disabled'

export interface ProjectMemberInfo {
  id: string
  name: string
  status: ProcessStatus
  pid: number | null
  enabled: boolean
}

export interface ProjectInfo {
  id: string
  kind: ProjectKind
  display_name: string
  note: string
  category: string
  web_port: number | null
  launch_uri: string | null
  enabled: boolean
  status: ProjectStatus
  process_count: number
  active_process_count: number
  cpu_percent: number
  memory_bytes: number
  members: ProjectMemberInfo[]
}

export interface ProjectPatch {
  kind?: ProjectKind
  display_name?: string
  note?: string
  category?: string
  web_port?: number
  launch_uri?: string
  enabled?: boolean
}

export interface ProjectActionMemberResult {
  process_id: string
  name: string
  success: boolean
  error: string | null
}

export interface ProjectActionResponse {
  project_id: string
  action: 'start' | 'stop' | 'restart'
  success: boolean
  persistence_error: string | null
  results: ProjectActionMemberResult[]
}

// @group Types > EnvFiles : Env file descriptor from the API
export interface EnvFileEntry {
  name: string
  path: string
}

// @group Types > Metrics : Single CPU + memory sample returned by the metrics history endpoint
export interface MetricSample {
  timestamp: string // ISO datetime
  cpu_percent: number
  memory_bytes: number
}

// @group Types > LogAlerts : Threshold-based stderr spike notification settings
export interface LogAlertConfig {
  enabled: boolean
  stderr_threshold: number
  cooldown_mins: number
  check_interval_mins: number
}

// @group Types > LogAlerts : Partial override for namespace or process scope (all fields optional = inherit)
export interface LogAlertOverride {
  enabled?: boolean
  stderr_threshold?: number
  cooldown_mins?: number
}

// @group Types > LogAlerts : Full store — global config + per-namespace overrides
export interface LogAlertStore {
  global: LogAlertConfig
  namespaces: Record<string, LogAlertOverride>
}

// @group Types > Update : Update availability info returned by GET /system/update/check
export interface UpdateInfo {
  current: string
  latest: string
  up_to_date: boolean
  download_url: string | null
  asset_name: string | null
  sha256: string | null
  integrity_verified: boolean
  /** True when the download is a platform installer (.exe setup, .deb) rather than a raw binary */
  is_installer: boolean
  release_notes: string | null
  published_at: string | null
  error?: string
}

// @group Types > Git : Git repository info for a process working directory
export interface GitInfo {
  is_git_repo: boolean
  branch?: string
  sha?: string
  sha_short?: string
  message?: string
  author?: string
  date?: string
  dirty: boolean
  ahead: number
  behind: number
  upstream_available: boolean
  ahead_behind_error: string | null
  pkg_manager: string
}

// @group Types > Git : Result of a git pull + dependency install operation
export interface GitPullResult {
  success: boolean
  pull_output: string
  deps_output: string | null
  pkg_manager: string
}

// @group Types > LogStats : One 5-minute bucket of stdout + stderr line counts (from disk)
export interface LogStatsBucket {
  window_start: string // RFC3339 UTC start of the 5-minute window
  stdout_count: number
  stderr_count: number
}

// @group Types > System : Bounded runtime system-statistics response
export interface SystemStats {
  cpu_percent: number
  ram_used_bytes: number
  ram_total_bytes: number
  gpu: {
    name: string
    utilization_percent: number
    vram_used_bytes: number
    vram_total_bytes: number
  } | null
}

// @group Types > Tunnels : Cloudflare / ngrok / custom tunnel management

export type TunnelProvider = 'cloudflare' | 'ngrok' | 'custom'

export type TunnelStatus = 'starting' | 'active' | 'failed' | 'stopped'

export interface TunnelEntry {
  id: string
  port: number
  process_name: string | null
  process_id: string | null
  provider: TunnelProvider
  public_url: string | null
  status: TunnelStatus
  error: string | null
  created_at: string
}

export interface CloudflareSettings {
  token?: string | null
}

export interface NgrokSettings {
  auth_token?: string | null
}

export interface CustomTunnelSettings {
  binary_path: string
  args_template: string
}

export interface TunnelSettings {
  provider: TunnelProvider
  cloudflare: CloudflareSettings
  ngrok: NgrokSettings
  custom: CustomTunnelSettings
}
