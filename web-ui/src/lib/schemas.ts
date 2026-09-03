import type {
  DaemonHealth,
  EnvFileEntry,
  GitInfo,
  LogLine,
  LogStatsBucket,
  MetricSample,
  NotificationConfig,
  NotificationsStore,
  PortEntry,
  ProcessInfo,
  ProcessStatus,
  ProjectActionResponse,
  ProjectInfo,
  ProjectStatus,
  ScriptInfo,
  SystemStats,
} from '@/types'

const PROCESS_STATUSES = new Set<ProcessStatus>([
  'stopped',
  'starting',
  'running',
  'stopping',
  'crashed',
  'errored',
  'watching',
  'sleeping',
])
const PROJECT_STATUSES = new Set<ProjectStatus>([
  'desktop',
  'running',
  'partial',
  'stopped',
  'errored',
  'disabled',
])

function record(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function nullable(value: unknown, predicate: (candidate: unknown) => boolean): boolean {
  return value === null || predicate(value)
}

function boundedStringRecord(
  value: unknown,
  maxEntries: number,
  maxKeyLength: number,
  maxValueLength: number,
  maxTotalLength: number
): boolean {
  if (!record(value)) return false
  const entries = Object.entries(value)
  if (entries.length > maxEntries) return false
  let totalLength = 0
  for (const [key, entry] of entries) {
    if (key.length > maxKeyLength || typeof entry !== 'string' || entry.length > maxValueLength) {
      return false
    }
    totalLength += key.length + entry.length
    if (totalLength > maxTotalLength) return false
  }
  return true
}

function finiteNumber(value: unknown): value is number {
  return typeof value === 'number' && Number.isFinite(value)
}

function integer(value: unknown): value is number {
  return typeof value === 'number' && Number.isInteger(value)
}

function boundedString(value: unknown, maxLength = 8_192): value is string {
  return typeof value === 'string' && value.length <= maxLength
}

function isNotificationTarget(value: unknown, urlField: 'url' | 'webhook_url'): boolean {
  return (
    record(value) &&
    boundedString(value[urlField], 4_096) &&
    typeof value.enabled === 'boolean' &&
    (urlField !== 'webhook_url' || value.channel === undefined || boundedString(value.channel, 256))
  )
}

export function isNotificationConfig(value: unknown): value is NotificationConfig {
  if (!record(value) || !record(value.events)) return false
  const events = value.events
  return (
    typeof events.on_crash === 'boolean' &&
    typeof events.on_restart === 'boolean' &&
    typeof events.on_start === 'boolean' &&
    typeof events.on_stop === 'boolean' &&
    ['on_unhealthy', 'on_health_recovered', 'on_cron_run', 'on_cron_fail'].every(
      key => events[key] === undefined || typeof events[key] === 'boolean'
    ) &&
    (value.events_override === undefined || typeof value.events_override === 'boolean') &&
    (value.webhook === undefined || isNotificationTarget(value.webhook, 'url')) &&
    (value.slack === undefined || isNotificationTarget(value.slack, 'webhook_url')) &&
    (value.teams === undefined || isNotificationTarget(value.teams, 'webhook_url')) &&
    (value.discord === undefined || isNotificationTarget(value.discord, 'webhook_url'))
  )
}

export function isNotificationsStore(value: unknown): value is NotificationsStore {
  if (!record(value) || !isNotificationConfig(value.global) || !record(value.namespaces)) {
    return false
  }
  const namespaces = Object.entries(value.namespaces)
  return (
    namespaces.length <= 256 &&
    namespaces.every(
      ([namespace, config]) =>
        namespace.length > 0 && namespace.length <= 128 && isNotificationConfig(config)
    )
  )
}

export function isOperationResult(value: unknown): value is { success: boolean; message?: string } {
  return (
    record(value) &&
    typeof value.success === 'boolean' &&
    (value.message === undefined || boundedString(value.message, 4_096))
  )
}

export function isPortListPayload(value: unknown): value is { ports: PortEntry[] } {
  return (
    record(value) &&
    Array.isArray(value.ports) &&
    value.ports.length <= 5_000 &&
    value.ports.every(
      entry =>
        record(entry) &&
        integer(entry.port) &&
        entry.port >= 0 &&
        entry.port <= 65_535 &&
        boundedString(entry.protocol, 16) &&
        boundedString(entry.local_address, 256) &&
        boundedString(entry.remote_address, 256) &&
        boundedString(entry.state, 64) &&
        nullable(entry.pid, candidate => integer(candidate) && candidate >= 0) &&
        nullable(entry.process_name, candidate => boundedString(candidate, 1_024)) &&
        (entry.ancestor_pids === undefined ||
          (Array.isArray(entry.ancestor_pids) &&
            entry.ancestor_pids.length <= 64 &&
            entry.ancestor_pids.every(pid => integer(pid) && pid >= 0)))
    )
  )
}

export function isMetricsPayload(value: unknown): value is { samples: MetricSample[] } {
  return (
    record(value) &&
    Array.isArray(value.samples) &&
    value.samples.length <= 2_000 &&
    value.samples.every(
      sample =>
        record(sample) &&
        boundedString(sample.timestamp, 128) &&
        finiteNumber(sample.cpu_percent) &&
        sample.cpu_percent >= 0 &&
        finiteNumber(sample.memory_bytes) &&
        sample.memory_bytes >= 0
    )
  )
}

export function isLogStatsPayload(value: unknown): value is { buckets: LogStatsBucket[] } {
  return (
    record(value) &&
    Array.isArray(value.buckets) &&
    value.buckets.length <= 512 &&
    value.buckets.every(
      bucket =>
        record(bucket) &&
        boundedString(bucket.window_start, 128) &&
        integer(bucket.stdout_count) &&
        bucket.stdout_count >= 0 &&
        integer(bucket.stderr_count) &&
        bucket.stderr_count >= 0
    )
  )
}

export function isLogLinesPayload(value: unknown): value is { lines: LogLine[] } {
  return (
    record(value) &&
    Array.isArray(value.lines) &&
    value.lines.length <= 10_000 &&
    value.lines.every(
      line =>
        record(line) &&
        boundedString(line.timestamp, 128) &&
        (line.stream === 'stdout' || line.stream === 'stderr') &&
        boundedString(line.content, 256 * 1_024)
    )
  )
}

export function isLogDatesPayload(
  value: unknown
): value is { dates: string[]; has_current: boolean } {
  return (
    record(value) &&
    Array.isArray(value.dates) &&
    value.dates.length <= 3_660 &&
    value.dates.every(date => boundedString(date, 32)) &&
    typeof value.has_current === 'boolean'
  )
}

export function isProcessInfo(value: unknown): value is ProcessInfo {
  if (!record(value)) return false
  return (
    boundedString(value.id, 128) &&
    value.id.length > 0 &&
    nullable(value.project_id, candidate => boundedString(candidate, 128)) &&
    boundedString(value.name, 256) &&
    value.name.length > 0 &&
    boundedString(value.script, 4_096) &&
    Array.isArray(value.args) &&
    value.args.length <= 256 &&
    value.args.every(argument => boundedString(argument, 4_096)) &&
    nullable(value.cwd, candidate => boundedString(candidate, 4_096)) &&
    typeof value.status === 'string' &&
    PROCESS_STATUSES.has(value.status as ProcessStatus) &&
    nullable(value.pid, candidate => integer(candidate) && candidate >= 0) &&
    integer(value.restart_count) &&
    value.restart_count >= 0 &&
    nullable(value.uptime_secs, candidate => finiteNumber(candidate) && candidate >= 0) &&
    nullable(value.last_exit_code, integer) &&
    typeof value.autorestart === 'boolean' &&
    integer(value.max_restarts) &&
    value.max_restarts >= 0 &&
    typeof value.watch === 'boolean' &&
    boundedString(value.namespace, 256) &&
    value.namespace.length > 0 &&
    boundedString(value.created_at, 128) &&
    nullable(value.started_at, candidate => boundedString(candidate, 128)) &&
    nullable(value.stopped_at, candidate => boundedString(candidate, 128)) &&
    nullable(value.cron, candidate => boundedString(candidate, 256)) &&
    nullable(value.cron_next_run, candidate => boundedString(candidate, 128)) &&
    Array.isArray(value.cron_run_history) &&
    value.cron_run_history.length <= 100 &&
    value.cron_run_history.every(
      run =>
        record(run) &&
        typeof run.run_at === 'string' &&
        run.run_at.length <= 128 &&
        Number.isFinite(Date.parse(run.run_at)) &&
        nullable(run.exit_code, integer) &&
        finiteNumber(run.duration_secs) &&
        run.duration_secs >= 0
    ) &&
    nullable(value.cpu_percent, candidate => finiteNumber(candidate) && candidate >= 0) &&
    nullable(value.memory_bytes, candidate => finiteNumber(candidate) && candidate >= 0) &&
    boundedStringRecord(value.env, 1_024, 256, 64 * 1_024, 1024 * 1_024) &&
    typeof value.enabled === 'boolean'
  )
}

export function isProjectInfo(value: unknown): value is ProjectInfo {
  if (!record(value)) return false
  return (
    boundedString(value.id, 128) &&
    value.id.length > 0 &&
    (value.kind === 'managed' || value.kind === 'desktop') &&
    boundedString(value.display_name, 256) &&
    value.display_name.length > 0 &&
    boundedString(value.note, 4_096) &&
    boundedString(value.category, 256) &&
    nullable(
      value.web_port,
      candidate => integer(candidate) && candidate > 0 && candidate <= 65_535
    ) &&
    nullable(value.launch_uri, candidate => boundedString(candidate, 4_096)) &&
    typeof value.enabled === 'boolean' &&
    typeof value.status === 'string' &&
    PROJECT_STATUSES.has(value.status as ProjectStatus) &&
    integer(value.process_count) &&
    value.process_count >= 0 &&
    integer(value.active_process_count) &&
    value.active_process_count >= 0 &&
    value.active_process_count <= value.process_count &&
    finiteNumber(value.cpu_percent) &&
    value.cpu_percent >= 0 &&
    finiteNumber(value.memory_bytes) &&
    value.memory_bytes >= 0 &&
    Array.isArray(value.members) &&
    value.members.length <= 1_000 &&
    value.members.every(
      member =>
        record(member) &&
        boundedString(member.id, 128) &&
        member.id.length > 0 &&
        boundedString(member.name, 256) &&
        member.name.length > 0 &&
        typeof member.status === 'string' &&
        PROCESS_STATUSES.has(member.status as ProcessStatus) &&
        nullable(member.pid, candidate => integer(candidate) && candidate >= 0) &&
        typeof member.enabled === 'boolean'
    )
  )
}

export function isProcessListPayload(value: unknown): value is { processes: ProcessInfo[] } {
  return (
    record(value) &&
    Array.isArray(value.processes) &&
    value.processes.length <= 1_000 &&
    value.processes.every(isProcessInfo)
  )
}

export function isProjectListPayload(value: unknown): value is { projects: ProjectInfo[] } {
  return (
    record(value) &&
    Array.isArray(value.projects) &&
    value.projects.length <= 1_000 &&
    value.projects.every(isProjectInfo)
  )
}

export function isProjectActionResponse(value: unknown): value is ProjectActionResponse {
  return (
    record(value) &&
    typeof value.project_id === 'string' &&
    (value.action === 'start' || value.action === 'stop' || value.action === 'restart') &&
    typeof value.success === 'boolean' &&
    nullable(value.persistence_error, candidate => typeof candidate === 'string') &&
    Array.isArray(value.results) &&
    value.results.length <= 1_000 &&
    value.results.every(
      result =>
        record(result) &&
        typeof result.process_id === 'string' &&
        typeof result.name === 'string' &&
        typeof result.success === 'boolean' &&
        nullable(result.error, candidate => typeof candidate === 'string')
    )
  )
}

export function isScriptInfo(value: unknown): value is ScriptInfo {
  return (
    record(value) &&
    typeof value.name === 'string' &&
    value.name.length > 0 &&
    value.name.length <= 255 &&
    typeof value.path === 'string' &&
    value.path.length <= 4_096 &&
    typeof value.language === 'string' &&
    value.language.length <= 64 &&
    integer(value.size_bytes) &&
    value.size_bytes >= 0 &&
    typeof value.modified_at === 'string' &&
    value.modified_at.length <= 128
  )
}

export function isScriptListPayload(value: unknown): value is { scripts: ScriptInfo[] } {
  return (
    record(value) &&
    Array.isArray(value.scripts) &&
    value.scripts.length <= 1_000 &&
    value.scripts.every(isScriptInfo)
  )
}

export function isScriptDetailPayload(value: unknown): value is {
  name: string
  path: string
  content: string
  language: string
  interpreter: string | null
  prefix_args: string[]
} {
  return (
    record(value) &&
    typeof value.name === 'string' &&
    value.name.length > 0 &&
    value.name.length <= 255 &&
    typeof value.path === 'string' &&
    value.path.length <= 4_096 &&
    typeof value.content === 'string' &&
    value.content.length <= 1024 * 1024 &&
    typeof value.language === 'string' &&
    value.language.length <= 64 &&
    nullable(value.interpreter, candidate => typeof candidate === 'string') &&
    Array.isArray(value.prefix_args) &&
    value.prefix_args.length <= 64 &&
    value.prefix_args.every(argument => typeof argument === 'string' && argument.length <= 4_096)
  )
}

export function isGitInfo(value: unknown): value is GitInfo {
  if (!record(value)) return false
  const optionalStrings = ['branch', 'sha', 'sha_short', 'message', 'author', 'date'] as const
  return (
    typeof value.is_git_repo === 'boolean' &&
    typeof value.dirty === 'boolean' &&
    integer(value.ahead) &&
    value.ahead >= 0 &&
    integer(value.behind) &&
    value.behind >= 0 &&
    typeof value.upstream_available === 'boolean' &&
    nullable(
      value.ahead_behind_error,
      candidate => typeof candidate === 'string' && candidate.length <= 1_024
    ) &&
    typeof value.pkg_manager === 'string' &&
    value.pkg_manager.length <= 64 &&
    optionalStrings.every(
      key =>
        value[key] === undefined || (typeof value[key] === 'string' && value[key].length <= 8_192)
    )
  )
}

export function isDaemonHealth(value: unknown): value is DaemonHealth {
  if (!record(value)) return false
  return (
    (value.status === 'ok' || value.status === 'degraded') &&
    typeof value.version === 'string' &&
    finiteNumber(value.uptime_secs) &&
    value.uptime_secs >= 0 &&
    integer(value.process_count) &&
    value.process_count >= 0 &&
    typeof value.persistence_healthy === 'boolean' &&
    nullable(value.persistence_error, candidate => typeof candidate === 'string')
  )
}

export function isSystemStats(value: unknown): value is SystemStats {
  if (!record(value)) return false
  const gpu = value.gpu
  return (
    finiteNumber(value.cpu_percent) &&
    value.cpu_percent >= 0 &&
    value.cpu_percent <= 100 &&
    finiteNumber(value.ram_used_bytes) &&
    value.ram_used_bytes >= 0 &&
    finiteNumber(value.ram_total_bytes) &&
    value.ram_total_bytes > 0 &&
    value.ram_used_bytes <= value.ram_total_bytes &&
    (gpu === null ||
      (record(gpu) &&
        typeof gpu.name === 'string' &&
        finiteNumber(gpu.utilization_percent) &&
        gpu.utilization_percent >= 0 &&
        gpu.utilization_percent <= 100 &&
        finiteNumber(gpu.vram_used_bytes) &&
        gpu.vram_used_bytes >= 0 &&
        finiteNumber(gpu.vram_total_bytes) &&
        gpu.vram_total_bytes > 0 &&
        gpu.vram_used_bytes <= gpu.vram_total_bytes))
  )
}

export function isEnvFileEntries(value: unknown): value is EnvFileEntry[] {
  return (
    Array.isArray(value) &&
    value.length <= 200 &&
    value.every(
      entry =>
        record(entry) &&
        typeof entry.name === 'string' &&
        entry.name.length > 0 &&
        entry.name.length <= 255 &&
        typeof entry.path === 'string' &&
        entry.path.length > 0 &&
        entry.path.length <= 4_096
    )
  )
}

export function isEnvFileContent(
  value: unknown
): value is { content: string; exists: boolean; filename?: string } {
  return (
    record(value) &&
    typeof value.content === 'string' &&
    value.content.length <= 1024 * 1024 &&
    typeof value.exists === 'boolean' &&
    (value.filename === undefined ||
      (typeof value.filename === 'string' && value.filename.length <= 255))
  )
}

export interface StreamTicketPayload {
  ticket: string
  expires_at: string
}

export interface AuthStatusPayload {
  password_configured: boolean
  pin_configured: boolean
  lock_timeout_mins: number | null
}

export interface AuthSessionPayload {
  valid: boolean
}

export interface LoginPayload {
  session_token: string
  expires_at: string
}

export function isAuthStatus(value: unknown): value is AuthStatusPayload {
  return (
    record(value) &&
    typeof value.password_configured === 'boolean' &&
    typeof value.pin_configured === 'boolean' &&
    nullable(
      value.lock_timeout_mins,
      candidate =>
        integer(candidate) && (candidate as number) >= 0 && (candidate as number) <= 10_080
    )
  )
}

export function isAuthSession(value: unknown): value is AuthSessionPayload {
  return record(value) && typeof value.valid === 'boolean'
}

export function isLoginPayload(value: unknown): value is LoginPayload {
  return (
    record(value) &&
    typeof value.session_token === 'string' &&
    value.session_token.length >= 16 &&
    value.session_token.length <= 512 &&
    typeof value.expires_at === 'string' &&
    value.expires_at.length <= 128 &&
    Number.isFinite(Date.parse(value.expires_at))
  )
}

export function isStreamTicket(value: unknown): value is StreamTicketPayload {
  if (!record(value)) return false
  return (
    typeof value.ticket === 'string' &&
    value.ticket.length >= 16 &&
    value.ticket.length <= 512 &&
    typeof value.expires_at === 'string' &&
    value.expires_at.length <= 128 &&
    Number.isFinite(Date.parse(value.expires_at))
  )
}
