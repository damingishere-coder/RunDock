// @group APIEndpoints : All fetch calls to the alter daemon REST API

import type {
  CronRun,
  DaemonHealth,
  EnvFileEntry,
  GitInfo,
  GitPullResult,
  LogAlertOverride,
  LogAlertStore,
  LogLine,
  LogStatsBucket,
  MetricSample,
  NotificationConfig,
  NotificationsStore,
  PortEntry,
  ProcessInfo,
  ProjectActionResponse,
  ProjectInfo,
  ProjectPatch,
  ScriptInfo,
  StartProcessBody,
  SystemStats,
  TunnelEntry,
  TunnelProvider,
  TunnelSettings,
  UpdateInfo,
  UpdateProcessBody,
} from '@/types'
import {
  isAuthSession,
  isAuthStatus,
  isDaemonHealth,
  isEnvFileContent,
  isEnvFileEntries,
  isGitInfo,
  isLoginPayload,
  isLogDatesPayload,
  isLogLinesPayload,
  isLogStatsPayload,
  isMetricsPayload,
  isNotificationsStore,
  isOperationResult,
  isPortListPayload,
  isProcessInfo,
  isProcessListPayload,
  isProjectActionResponse,
  isProjectInfo,
  isProjectListPayload,
  isScriptDetailPayload,
  isScriptListPayload,
  isStreamTicket,
  isSystemStats,
  type AuthSessionPayload,
  type AuthStatusPayload,
  type LoginPayload,
  type StreamTicketPayload,
} from '@/lib/schemas'
import {
  captureDaemonTarget,
  daemonFetch,
  readResponseTextBounded,
  type DaemonTarget,
} from '@/lib/transport'

export interface BulkNamespaceResult {
  status: 'complete' | 'partial' | 'empty'
  namespace: string
  attempted: number
  succeeded: number
  failed: number
  started?: number
  stopped?: number
  restarted?: number
  processes: ProcessInfo[]
  failures: Array<{ id: string; error: string }>
  persistence: { status: 'committed' | 'failed'; error: string | null }
}

// @group Types > AI : Chat message and request types (mirrored from Rust models/ai.rs)
export interface AiChatMessage {
  role: 'user' | 'assistant'
  content: string
}

export interface AiChatRequest {
  message: string
  process_id?: string
  history: AiChatMessage[]
  model?: string
  provider?: string
}

export interface AiSettingsInfo {
  provider: string
  enabled: boolean
  model: string
  // GitHub
  github_token_set: boolean
  github_token_hint: string
  github_username: string
  client_id_set: boolean
  client_id_builtin: boolean
  // Claude
  anthropic_key_set: boolean
  anthropic_key_hint: string
  // OpenAI
  openai_key_set: boolean
  openai_key_hint: string
  openai_base_url: string
  // Ollama
  ollama_base_url: string
}

export interface AiAuthStartResponse {
  flow_id: string
  poll_token: string
  user_code: string
  verification_uri: string
  expires_in: number
  interval: number
}

export interface AiAuthStatusResponse {
  status: 'idle' | 'pending' | 'expired' | 'denied' | 'complete' | 'error'
  username?: string
  interval?: number
  message?: string
}

export interface AiModelInfo {
  id: string
  label: string
  publisher: string
}

type Targeted<T> = T & { target: DaemonTarget }

const AI_STREAM_MAX_BYTES = 4 * 1024 * 1024
const AI_STREAM_MAX_LINE_CHARS = 256 * 1024
const AI_STREAM_MAX_OUTPUT_CHARS = 1_000_000
const AI_STREAM_IDLE_TIMEOUT_MS = 30_000
const AI_STREAM_TOTAL_TIMEOUT_MS = 120_000

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function isBoundedString(value: unknown, maxLength = 4_096): value is string {
  return typeof value === 'string' && value.length <= maxLength
}

function isAiSettingsInfo(value: unknown): value is AiSettingsInfo {
  if (!isRecord(value)) return false
  return (
    isBoundedString(value.provider, 64) &&
    typeof value.enabled === 'boolean' &&
    isBoundedString(value.model, 256) &&
    typeof value.github_token_set === 'boolean' &&
    isBoundedString(value.github_token_hint, 256) &&
    isBoundedString(value.github_username, 256) &&
    typeof value.client_id_set === 'boolean' &&
    typeof value.client_id_builtin === 'boolean' &&
    typeof value.anthropic_key_set === 'boolean' &&
    isBoundedString(value.anthropic_key_hint, 256) &&
    typeof value.openai_key_set === 'boolean' &&
    isBoundedString(value.openai_key_hint, 256) &&
    isBoundedString(value.openai_base_url) &&
    isBoundedString(value.ollama_base_url)
  )
}

function isAiModelsPayload(value: unknown): value is { models: AiModelInfo[] } {
  return (
    isRecord(value) &&
    Array.isArray(value.models) &&
    value.models.length <= 1_000 &&
    value.models.every(
      model =>
        isRecord(model) &&
        isBoundedString(model.id, 256) &&
        model.id.length > 0 &&
        isBoundedString(model.label, 256) &&
        isBoundedString(model.publisher, 256)
    )
  )
}

function isAiAuthStartResponse(value: unknown): value is AiAuthStartResponse {
  return (
    isRecord(value) &&
    /^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(
      String(value.flow_id)
    ) &&
    isBoundedString(value.poll_token, 64) &&
    /^[0-9a-f]{64}$/i.test(value.poll_token) &&
    isBoundedString(value.user_code, 64) &&
    value.user_code.length > 0 &&
    value.verification_uri === 'https://github.com/login/device' &&
    Number.isInteger(value.expires_in) &&
    Number(value.expires_in) >= 60 &&
    Number(value.expires_in) <= 30 * 60 &&
    Number.isInteger(value.interval) &&
    Number(value.interval) >= 1 &&
    Number(value.interval) <= 60
  )
}

function isAiAuthStatusResponse(value: unknown): value is AiAuthStatusResponse {
  if (!isRecord(value)) return false
  const knownStatus =
    value.status === 'idle' ||
    value.status === 'pending' ||
    value.status === 'expired' ||
    value.status === 'denied' ||
    value.status === 'complete' ||
    value.status === 'error'
  return (
    knownStatus &&
    (value.username === undefined || isBoundedString(value.username, 256)) &&
    (value.interval === undefined ||
      (Number.isInteger(value.interval) &&
        Number(value.interval) >= 1 &&
        Number(value.interval) <= 60)) &&
    (value.message === undefined || isBoundedString(value.message, 4_096)) &&
    (value.status !== 'complete' ||
      (typeof value.username === 'string' && value.username.length > 0))
  )
}

interface TelegramConfigInfo {
  enabled: boolean
  bot_token_hint: string | null
  bot_token_set: boolean
  allowed_chat_ids: number[]
  notify_on_crash: boolean
  notify_on_start: boolean
  notify_on_stop: boolean
  notify_on_restart: boolean
}

interface TelegramBotInfo {
  ok: boolean
  username: string | null
  first_name: string | null
  error: string | null
}

function isTelegramConfigInfo(value: unknown): value is TelegramConfigInfo {
  return (
    isRecord(value) &&
    typeof value.enabled === 'boolean' &&
    (value.bot_token_hint === null || isBoundedString(value.bot_token_hint, 256)) &&
    typeof value.bot_token_set === 'boolean' &&
    Array.isArray(value.allowed_chat_ids) &&
    value.allowed_chat_ids.length <= 100 &&
    value.allowed_chat_ids.every(id => Number.isSafeInteger(id) && Number(id) !== 0) &&
    typeof value.notify_on_crash === 'boolean' &&
    typeof value.notify_on_start === 'boolean' &&
    typeof value.notify_on_stop === 'boolean' &&
    typeof value.notify_on_restart === 'boolean'
  )
}

function isTelegramBotInfo(value: unknown): value is TelegramBotInfo {
  return (
    isRecord(value) &&
    typeof value.ok === 'boolean' &&
    (value.username === null || isBoundedString(value.username, 256)) &&
    (value.first_name === null || isBoundedString(value.first_name, 256)) &&
    (value.error === null || isBoundedString(value.error, 4_096))
  )
}

function isTunnelEntry(value: unknown): value is TunnelEntry {
  if (!isRecord(value)) return false
  return (
    isBoundedString(value.id, 256) &&
    value.id.length > 0 &&
    Number.isInteger(value.port) &&
    Number(value.port) >= 1 &&
    Number(value.port) <= 65_535 &&
    (value.process_name === null || isBoundedString(value.process_name, 256)) &&
    (value.process_id === null || isBoundedString(value.process_id, 256)) &&
    (value.provider === 'cloudflare' ||
      value.provider === 'ngrok' ||
      value.provider === 'custom') &&
    (value.public_url === null || isBoundedString(value.public_url)) &&
    (value.status === 'starting' ||
      value.status === 'active' ||
      value.status === 'failed' ||
      value.status === 'stopped') &&
    (value.error === null || isBoundedString(value.error, 8_192)) &&
    isBoundedString(value.created_at, 128)
  )
}

function isTunnelListPayload(value: unknown): value is { tunnels: TunnelEntry[] } {
  return (
    isRecord(value) &&
    Array.isArray(value.tunnels) &&
    value.tunnels.length <= 1_000 &&
    value.tunnels.every(isTunnelEntry)
  )
}

function isTunnelCreateResult(value: unknown): value is { tunnel?: TunnelEntry; error?: string } {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return false
  const result = value as Record<string, unknown>
  const hasTunnel = isTunnelEntry(result.tunnel)
  const hasError = typeof result.error === 'string' && result.error.length <= 8_192
  return hasTunnel !== hasError
}

function isTunnelSettings(value: unknown): value is TunnelSettings {
  if (!isRecord(value)) return false
  const cloudflare = value.cloudflare
  const ngrok = value.ngrok
  const custom = value.custom
  return (
    (value.provider === 'cloudflare' ||
      value.provider === 'ngrok' ||
      value.provider === 'custom') &&
    isRecord(cloudflare) &&
    (cloudflare.token === undefined ||
      cloudflare.token === null ||
      isBoundedString(cloudflare.token, 4_096)) &&
    isRecord(ngrok) &&
    (ngrok.auth_token === undefined ||
      ngrok.auth_token === null ||
      isBoundedString(ngrok.auth_token, 4_096)) &&
    isRecord(custom) &&
    isBoundedString(custom.binary_path) &&
    isBoundedString(custom.args_template, 16_384)
  )
}

async function readAiStreamChunk(
  reader: ReadableStreamDefaultReader<Uint8Array>,
  timeoutMs: number,
  timeoutMessage: string,
  signal: AbortSignal
): Promise<ReadableStreamReadResult<Uint8Array>> {
  let timeoutId: ReturnType<typeof setTimeout> | undefined
  let abortListener: (() => void) | undefined
  try {
    return await Promise.race([
      reader.read(),
      new Promise<never>((_resolve, reject) => {
        timeoutId = setTimeout(() => reject(new Error(timeoutMessage)), timeoutMs)
      }),
      new Promise<never>((_, reject) => {
        abortListener = () =>
          reject(
            signal.reason instanceof Error
              ? signal.reason
              : new DOMException('请求已取消', 'AbortError')
          )
        if (signal.aborted) abortListener()
        else signal.addEventListener('abort', abortListener, { once: true })
      }),
    ])
  } finally {
    if (timeoutId !== undefined) clearTimeout(timeoutId)
    if (abortListener) signal.removeEventListener('abort', abortListener)
  }
}

async function request<T>(path: string, init?: RequestInit, target?: DaemonTarget): Promise<T> {
  const headers = new Headers(init?.headers)
  if (!headers.has('Content-Type')) headers.set('Content-Type', 'application/json')
  const res = await daemonFetch(
    path,
    {
      ...init,
      headers,
    },
    target
  )
  if (!res.ok) {
    let body: string
    try {
      body = await readResponseTextBounded(res, 8 * 1_024, init?.signal)
    } catch (error: unknown) {
      if (init?.signal?.aborted || (error as Error)?.name === 'AbortError') throw error
      throw new Error(`HTTP ${res.status} 错误响应超过大小上限`)
    }
    let message = body.trim()
    try {
      const data = JSON.parse(body) as { error?: string }
      message = data.error ?? message
    } catch {
      // Preserve a bounded plain-text server error for diagnostics.
    }
    throw new Error(message || `HTTP ${res.status}`)
  }
  if (res.status === 204) return undefined as T

  const body = await readResponseTextBounded(res, 16 * 1_024 * 1_024, init?.signal)
  if (!body.trim()) throw new Error(`HTTP ${res.status} 响应缺少 JSON 正文`)
  try {
    return JSON.parse(body) as T
  } catch {
    throw new Error(`HTTP ${res.status} 返回了无效 JSON`)
  }
}

async function validatedRequest<T>(
  path: string,
  validator: (value: unknown) => value is T,
  invalidMessage: string,
  init?: RequestInit
): Promise<T> {
  const result = await request<unknown>(path, init)
  if (!validator(result)) throw new Error(invalidMessage)
  return result
}

function createStreamTicketForTarget(
  target: DaemonTarget,
  path: string,
  query?: string,
  init?: RequestInit
): Promise<StreamTicketPayload> {
  return request<unknown>(
    '/stream-ticket',
    { ...init, method: 'POST', body: JSON.stringify({ path, query }) },
    target
  ).then(payload => {
    if (!isStreamTicket(payload)) throw new Error('服务端返回了无效的流式访问凭据')
    return payload
  })
}

function isBulkNamespaceResult(value: unknown): value is BulkNamespaceResult {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return false
  const result = value as Record<string, unknown>
  if (!['complete', 'partial', 'empty'].includes(String(result.status))) return false
  if (
    typeof result.namespace !== 'string' ||
    !Number.isInteger(result.attempted) ||
    !Number.isInteger(result.succeeded) ||
    !Number.isInteger(result.failed) ||
    !Array.isArray(result.processes) ||
    !Array.isArray(result.failures) ||
    !result.failures.every(
      failure =>
        !!failure &&
        typeof failure === 'object' &&
        typeof (failure as Record<string, unknown>).id === 'string' &&
        typeof (failure as Record<string, unknown>).error === 'string'
    ) ||
    !result.persistence ||
    typeof result.persistence !== 'object'
  ) {
    return false
  }
  const persistence = result.persistence as Record<string, unknown>
  return (
    (persistence.status === 'committed' || persistence.status === 'failed') &&
    (persistence.error === null || typeof persistence.error === 'string')
  )
}

async function namespaceRequest(path: string): Promise<BulkNamespaceResult> {
  const result = await request<unknown>(path, { method: 'POST' })
  if (!isBulkNamespaceResult(result)) throw new Error('服务端返回了无效的命名空间操作结果')
  return result
}

async function notificationMutation(
  path: string,
  init: RequestInit
): Promise<{ success: true; message?: string }> {
  return operationMutation(path, init, '通知操作')
}

async function operationMutation(
  path: string,
  init: RequestInit,
  label: string
): Promise<{ success: true; message?: string }> {
  const result = await validatedRequest(
    path,
    isOperationResult,
    `服务端返回了无效的${label}结果`,
    init
  )
  if (!result.success) throw new Error(result.message ?? `${label}失败`)
  return { ...result, success: true }
}

// @group APIEndpoints > Processes
export const api = {
  getPorts: (init?: RequestInit): Promise<{ ports: PortEntry[] }> =>
    validatedRequest('/ports', isPortListPayload, '服务端返回了无效的端口列表', init),

  killPort: (
    pid: number,
    expected: { port: number; process_name: string | null }
  ): Promise<{ success: boolean; error?: string }> =>
    request(`/ports/kill/${pid}`, { method: 'POST', body: JSON.stringify(expected) }),

  getProcesses: (init?: RequestInit): Promise<{ processes: ProcessInfo[] }> =>
    validatedRequest('/processes', isProcessListPayload, '服务端返回了无效的进程列表', init),

  getProcess: (id: string, init?: RequestInit): Promise<ProcessInfo> =>
    validatedRequest(`/processes/${id}`, isProcessInfo, '服务端返回了无效的进程详情', init),

  // @group APIEndpoints > Projects : Logical project aggregation and lifecycle
  getProjects: (init?: RequestInit): Promise<{ projects: ProjectInfo[] }> =>
    validatedRequest('/projects', isProjectListPayload, '服务端返回了无效的项目列表', init),

  getProject: (id: string, init?: RequestInit): Promise<ProjectInfo> =>
    validatedRequest(`/projects/${id}`, isProjectInfo, '服务端返回了无效的项目详情', init),

  updateProject: (id: string, body: ProjectPatch): Promise<ProjectInfo> =>
    validatedRequest(`/projects/${id}`, isProjectInfo, '服务端返回了无效的项目详情', {
      method: 'PATCH',
      body: JSON.stringify(body),
    }),

  startProject: (id: string): Promise<ProjectActionResponse> =>
    validatedRequest(
      `/projects/${id}/start`,
      isProjectActionResponse,
      '服务端返回了无效的项目操作结果',
      { method: 'POST' }
    ),

  stopProject: (id: string): Promise<ProjectActionResponse> =>
    validatedRequest(
      `/projects/${id}/stop`,
      isProjectActionResponse,
      '服务端返回了无效的项目操作结果',
      { method: 'POST' }
    ),

  restartProject: (id: string): Promise<ProjectActionResponse> =>
    validatedRequest(
      `/projects/${id}/restart`,
      isProjectActionResponse,
      '服务端返回了无效的项目操作结果',
      { method: 'POST' }
    ),

  assignProcessProject: (processId: string, projectId: string): Promise<ProcessInfo> =>
    validatedRequest(
      `/processes/${processId}/project`,
      isProcessInfo,
      '服务端返回了无效的进程详情',
      {
        method: 'PATCH',
        body: JSON.stringify({ project_id: projectId }),
      }
    ),

  startProcess: (body: StartProcessBody): Promise<ProcessInfo> =>
    validatedRequest('/processes', isProcessInfo, '服务端返回了无效的进程详情', {
      method: 'POST',
      body: JSON.stringify(body),
    }),

  stopProcess: (id: string): Promise<ProcessInfo> =>
    validatedRequest(`/processes/${id}/stop`, isProcessInfo, '服务端返回了无效的进程详情', {
      method: 'POST',
    }),

  startStopped: (id: string): Promise<ProcessInfo> =>
    validatedRequest(`/processes/${id}/start`, isProcessInfo, '服务端返回了无效的进程详情', {
      method: 'POST',
    }),

  restartProcess: (id: string): Promise<ProcessInfo> =>
    validatedRequest(`/processes/${id}/restart`, isProcessInfo, '服务端返回了无效的进程详情', {
      method: 'POST',
    }),

  // @group APIEndpoints > Namespace : Bulk namespace operations — one aggregated notification each
  startNamespace: (ns: string): Promise<BulkNamespaceResult> =>
    namespaceRequest(`/processes/namespace/${encodeURIComponent(ns)}/start`),

  stopNamespace: (ns: string): Promise<BulkNamespaceResult> =>
    namespaceRequest(`/processes/namespace/${encodeURIComponent(ns)}/stop`),

  restartNamespace: (ns: string): Promise<BulkNamespaceResult> =>
    namespaceRequest(`/processes/namespace/${encodeURIComponent(ns)}/restart`),

  deleteProcess: (id: string): Promise<void> => request(`/processes/${id}`, { method: 'DELETE' }),

  cloneProcess: (id: string, name?: string): Promise<ProcessInfo> =>
    validatedRequest(`/processes/${id}/clone`, isProcessInfo, '服务端返回了无效的进程详情', {
      method: 'POST',
      body: JSON.stringify(name ? { name } : {}),
    }),

  updateProcess: (id: string, body: UpdateProcessBody): Promise<ProcessInfo> =>
    validatedRequest(`/processes/${id}`, isProcessInfo, '服务端返回了无效的进程详情', {
      method: 'PATCH',
      body: JSON.stringify(body),
    }),

  updateProcessNotifications: (id: string, notify: NotificationConfig): Promise<ProcessInfo> =>
    validatedRequest(
      `/processes/${id}/notifications`,
      isProcessInfo,
      '服务端返回了无效的进程详情',
      {
        method: 'PATCH',
        body: JSON.stringify({ notify }),
      }
    ),

  setProcessEnabled: (id: string, enabled: boolean): Promise<ProcessInfo> =>
    validatedRequest(`/processes/${id}/enabled`, isProcessInfo, '服务端返回了无效的进程详情', {
      method: 'PATCH',
      body: JSON.stringify({ enabled }),
    }),

  resetProcess: (id: string): Promise<ProcessInfo> =>
    validatedRequest(`/processes/${id}/reset`, isProcessInfo, '服务端返回了无效的进程详情', {
      method: 'POST',
    }),

  openTerminal: (id: string): Promise<void> =>
    request(`/processes/${id}/terminal`, { method: 'POST' }),

  openFolder: (path: string): Promise<void> =>
    request(`/system/open-folder`, { method: 'POST', body: JSON.stringify({ path }) }),

  // @group APIEndpoints > Metrics : Rolling CPU + memory history for a process
  getMetricsHistory: (id: string, init?: RequestInit): Promise<{ samples: MetricSample[] }> =>
    validatedRequest(
      `/processes/${id}/metrics/history`,
      isMetricsPayload,
      '服务端返回了无效的指标历史',
      init
    ),

  // @group APIEndpoints > LogStats : 5-minute stdout/stderr log count buckets for a process
  getLogStats: (id: string, init?: RequestInit): Promise<{ buckets: LogStatsBucket[] }> =>
    validatedRequest(
      `/processes/${id}/logs/stats`,
      isLogStatsPayload,
      '服务端返回了无效的日志统计',
      init
    ),

  // @group APIEndpoints > LogAlerts : Get / update the log-spike alert store (global + namespace overrides)
  getLogAlerts: (): Promise<LogAlertStore> => request('/log-alerts'),

  updateLogAlerts: (store: LogAlertStore): Promise<LogAlertStore> =>
    request('/log-alerts', { method: 'PUT', body: JSON.stringify(store) }),

  putLogAlertNamespace: (ns: string, override_: LogAlertOverride): Promise<LogAlertOverride> =>
    request(`/log-alerts/namespace/${encodeURIComponent(ns)}`, {
      method: 'PUT',
      body: JSON.stringify(override_),
    }),

  deleteLogAlertNamespace: (ns: string): Promise<void> =>
    request(`/log-alerts/namespace/${encodeURIComponent(ns)}`, { method: 'DELETE' }),

  // @group APIEndpoints > Logs
  getLogs: (
    id: string,
    params?: { lines?: number; date?: string },
    init?: RequestInit
  ): Promise<{ lines: LogLine[] }> => {
    const qs = new URLSearchParams()
    if (params?.lines) qs.set('lines', String(params.lines))
    if (params?.date) qs.set('date', params.date)
    return validatedRequest(
      `/processes/${id}/logs?${qs}`,
      isLogLinesPayload,
      '服务端返回了无效的日志内容',
      init
    )
  },

  getLogDates: (
    id: string,
    init?: RequestInit
  ): Promise<{ dates: string[]; has_current: boolean }> =>
    validatedRequest(
      `/processes/${id}/logs/dates`,
      isLogDatesPayload,
      '服务端返回了无效的日志日期列表',
      init
    ),

  deleteLogs: (id: string): Promise<{ success: true; message?: string }> =>
    operationMutation(`/processes/${id}/logs`, { method: 'DELETE' }, '日志删除'),

  // @group APIEndpoints > EnvFiles : Process-scoped env file operations
  listEnvFiles: async (id: string, init?: RequestInit): Promise<{ files: EnvFileEntry[] }> => {
    const result = await request<unknown>(`/processes/${id}/envfiles`, init)
    if (
      !result ||
      typeof result !== 'object' ||
      !isEnvFileEntries((result as { files?: unknown }).files)
    ) {
      throw new Error('服务端返回了无效的环境文件列表')
    }
    return result as { files: EnvFileEntry[] }
  },

  getEnvFile: (
    id: string,
    filename = '.env',
    init?: RequestInit
  ): Promise<{ content: string; exists: boolean; filename: string }> =>
    request<unknown>(
      `/processes/${id}/envfile?filename=${encodeURIComponent(filename)}`,
      init
    ).then(result => {
      if (!isEnvFileContent(result) || typeof result.filename !== 'string') {
        throw new Error('服务端返回了无效的环境文件内容')
      }
      return result as { content: string; exists: boolean; filename: string }
    }),

  saveEnvFile: (
    id: string,
    content: string,
    filename = '.env'
  ): Promise<{ success: boolean; path: string; filename: string }> =>
    request(`/processes/${id}/envfile`, {
      method: 'PUT',
      body: JSON.stringify({ content, filename }),
    }),

  // @group APIEndpoints > EnvFiles : Path-scoped env file operations (for StartPage/EditPage)
  listEnvPath: async (dir: string): Promise<{ files: EnvFileEntry[] }> => {
    const result = await request<unknown>(`/system/list-env?path=${encodeURIComponent(dir)}`)
    if (
      !result ||
      typeof result !== 'object' ||
      !isEnvFileEntries((result as { files?: unknown }).files)
    ) {
      throw new Error('服务端返回了无效的环境文件列表')
    }
    return result as { files: EnvFileEntry[] }
  },

  readEnvFile: (filePath: string): Promise<{ content: string; exists: boolean }> =>
    request<unknown>(`/system/read-env?path=${encodeURIComponent(filePath)}`).then(result => {
      if (!isEnvFileContent(result)) throw new Error('服务端返回了无效的环境文件内容')
      return result
    }),

  writeEnvFile: (filePath: string, content: string): Promise<{ success: boolean; path: string }> =>
    request('/system/write-env', {
      method: 'POST',
      body: JSON.stringify({ path: filePath, content }),
    }),

  syncEnvFiles: (
    sourcePath: string
  ): Promise<{ success: boolean; synced_files: number; errors?: string[] }> =>
    request('/system/sync-env', {
      method: 'POST',
      body: JSON.stringify({ source_path: sourcePath }),
    }),

  getCronHistory: (id: string): Promise<{ runs: CronRun[] }> =>
    request(`/processes/${id}/cron/history`),

  createStreamTicket: (
    path: string,
    query?: string,
    init?: RequestInit
  ): Promise<StreamTicketPayload> =>
    createStreamTicketForTarget(captureDaemonTarget(), path, query, init),

  streamLogs: async (id: string, init?: RequestInit): Promise<EventSource> => {
    const path = `/processes/${id}/logs/stream`
    const target = captureDaemonTarget()
    const { ticket } = await createStreamTicketForTarget(target, path, undefined, init)
    return new EventSource(`${target.baseUrl}${path}?ticket=${encodeURIComponent(ticket)}`)
  },

  // @group APIEndpoints > Scripts
  saveScript: (body: {
    name: string
    language: string
    content: string
  }): Promise<{ path: string; name: string; filename: string; language: string }> =>
    request('/scripts', { method: 'POST', body: JSON.stringify(body) }),

  listScripts: (): Promise<{ scripts: ScriptInfo[] }> =>
    validatedRequest('/scripts', isScriptListPayload, '服务端返回了无效的脚本列表'),

  getScript: (
    name: string
  ): Promise<{
    name: string
    path: string
    content: string
    language: string
    interpreter: string | null
    prefix_args: string[]
  }> =>
    validatedRequest(
      `/scripts/${encodeURIComponent(name)}`,
      isScriptDetailPayload,
      '服务端返回了无效的脚本详情'
    ),

  deleteScript: (name: string): Promise<void> =>
    request(`/scripts/${encodeURIComponent(name)}`, { method: 'DELETE' }),

  runScript: async (name: string, init?: RequestInit): Promise<EventSource> => {
    const path = `/scripts/${encodeURIComponent(name)}/run`
    const target = captureDaemonTarget()
    const { ticket } = await createStreamTicketForTarget(target, path, undefined, init)
    return new EventSource(`${target.baseUrl}${path}?ticket=${encodeURIComponent(ticket)}`)
  },

  // @group APIEndpoints > Notifications
  getNotifications: (): Promise<NotificationsStore> =>
    validatedRequest('/notifications', isNotificationsStore, '服务端返回了无效的通知设置'),

  updateGlobalNotifications: (config: NotificationConfig): Promise<{ success: boolean }> =>
    notificationMutation('/notifications/global', { method: 'PUT', body: JSON.stringify(config) }),

  updateNamespaceNotifications: (
    ns: string,
    config: NotificationConfig
  ): Promise<{ success: boolean }> =>
    notificationMutation(`/notifications/namespace/${encodeURIComponent(ns)}`, {
      method: 'PUT',
      body: JSON.stringify(config),
    }),

  deleteNamespaceNotifications: (ns: string): Promise<{ success: boolean }> =>
    notificationMutation(`/notifications/namespace/${encodeURIComponent(ns)}`, {
      method: 'DELETE',
    }),

  testNotification: async (
    config: NotificationConfig
  ): Promise<{ success: boolean; message: string }> => {
    const result = await notificationMutation('/notifications/test', {
      method: 'POST',
      body: JSON.stringify(config),
    })
    if (!result.message) throw new Error('服务端返回的通知测试结果缺少说明')
    return { success: true, message: result.message }
  },

  // @group APIEndpoints > System
  getHealth: (init?: RequestInit): Promise<DaemonHealth> =>
    validatedRequest('/system/health', isDaemonHealth, '服务端返回了无效的健康状态', init),

  getSystemStats: async (init?: RequestInit): Promise<SystemStats> => {
    const payload = await request<unknown>('/system/stats', init)
    if (!isSystemStats(payload)) throw new Error('服务端返回了无效的系统状态数据')
    return payload
  },

  getSystemPaths: (): Promise<{ data_dir: string; log_dir: string }> => request('/system/paths'),

  checkEnvPath: (dir: string): Promise<{ exists: boolean; path: string }> =>
    request(`/system/check-env?path=${encodeURIComponent(dir)}`),

  browsePath: (
    dir: string,
    init?: RequestInit
  ): Promise<{
    path: string
    parent: string | null
    entries: { name: string; path: string; is_dir: boolean }[]
    truncated: boolean
  }> => request(`/system/browse?path=${encodeURIComponent(dir)}`, init),

  saveState: (): Promise<void> => request('/system/save', { method: 'POST' }),

  shutdownDaemon: (): Promise<void> => request('/system/shutdown', { method: 'POST' }),

  restartDaemon: (): Promise<void> => request('/system/restart', { method: 'POST' }),

  // @group APIEndpoints > AI : Get stored AI settings (token is masked server-side)
  aiGetSettings: async (init?: RequestInit): Promise<AiSettingsInfo> => {
    const result = await request<unknown>('/ai/settings', init)
    if (!isAiSettingsInfo(result)) throw new Error('服务端返回了无效的 AI 设置')
    return result
  },

  // @group APIEndpoints > AI : Persist AI settings (send empty string to keep existing secrets)
  aiSaveSettings: (body: {
    provider?: string
    enabled?: boolean
    model?: string
    client_id?: string
    github_token?: string
    anthropic_key?: string
    clear_anthropic_key?: boolean
    openai_key?: string
    clear_openai_key?: boolean
    openai_base_url?: string
    ollama_base_url?: string
  }): Promise<{ success: true; message?: string }> =>
    operationMutation('/ai/settings', { method: 'PUT', body: JSON.stringify(body) }, 'AI 设置保存'),

  // @group APIEndpoints > AI : Begin GitHub OAuth Device Flow — returns user_code to display
  aiAuthStart: (): Promise<AiAuthStartResponse> =>
    validatedRequest(
      '/ai/auth/start',
      isAiAuthStartResponse,
      '服务端返回了无效的 GitHub 登录信息',
      { method: 'POST' }
    ),

  // @group APIEndpoints > AI : Poll GitHub token exchange — returns current auth status
  aiAuthStatus: (
    flowId: string,
    pollToken: string,
    init?: RequestInit
  ): Promise<AiAuthStatusResponse> => {
    const headers = new Headers(init?.headers)
    headers.set('X-RunDock-Device-Token', pollToken)
    return validatedRequest(
      `/ai/auth/status?flow_id=${encodeURIComponent(flowId)}`,
      isAiAuthStatusResponse,
      '服务端返回了无效的 GitHub 登录状态',
      { ...init, headers }
    )
  },

  // @group APIEndpoints > AI : Disconnect GitHub account — clears stored token and username
  aiAuthLogout: (): Promise<{ success: true; message?: string }> =>
    operationMutation('/ai/auth', { method: 'DELETE' }, 'GitHub 退出登录'),

  // @group APIEndpoints > AI : List GitHub Models catalog (chat-completion models only)
  aiGetModels: async (
    provider?: string,
    init?: RequestInit
  ): Promise<{ models: AiModelInfo[] }> => {
    const result = await request<unknown>(
      `/ai/models${provider ? `?provider=${encodeURIComponent(provider)}` : ''}`,
      init
    )
    if (!isAiModelsPayload(result)) throw new Error('服务端返回了无效的 AI 模型列表')
    return result
  },

  // @group APIEndpoints > AI : Stream a chat response — returns AbortController to cancel
  aiChat(
    req: AiChatRequest,
    onDelta: (token: string) => void,
    onDone: () => void,
    onError: (msg: string) => void
  ): AbortController {
    const abort = new AbortController()
    ;(async () => {
      let reader: ReadableStreamDefaultReader<Uint8Array> | undefined
      try {
        const res = await daemonFetch('/ai/chat', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify(req),
          signal: abort.signal,
        })
        if (!res.ok) {
          let body: string
          try {
            body = await readResponseTextBounded(res, 8 * 1_024, abort.signal)
          } catch (error: unknown) {
            if (abort.signal.aborted || (error as Error)?.name === 'AbortError') throw error
            onError(`HTTP ${res.status} 错误响应超过大小上限`)
            return
          }
          try {
            const data = JSON.parse(body) as { error?: string }
            onError(data.error ?? (body.trim() || `HTTP ${res.status}`))
          } catch {
            onError(body.trim() || `HTTP ${res.status}`)
          }
          return
        }
        reader = res.body?.getReader()
        if (!reader) {
          onError('AI 响应流不可用')
          return
        }
        const decoder = new TextDecoder()
        let buf = ''
        let streamBytes = 0
        let outputChars = 0
        const streamDeadline = Date.now() + AI_STREAM_TOTAL_TIMEOUT_MS
        const consumeLine = (line: string): boolean => {
          const trimmed = line.trim()
          if (!trimmed.startsWith('data:')) return false
          const payload = trimmed.slice(5).trimStart()
          try {
            const parsed: unknown = JSON.parse(payload)
            if (!isRecord(parsed) || Object.keys(parsed).length !== 1) {
              onError('AI 响应流包含无效事件')
              return true
            }
            if ('error' in parsed) {
              if (typeof parsed.error !== 'string' || parsed.error.length === 0) {
                onError('AI 响应流包含无效错误字段')
                return true
              }
              onError(parsed.error)
              return true
            }
            if ('done' in parsed) {
              if (parsed.done !== true) {
                onError('AI 响应流包含无效完成字段')
                return true
              }
              onDone()
              return true
            }
            if ('delta' in parsed) {
              if (typeof parsed.delta !== 'string') {
                onError('AI 响应流包含无效文本字段')
                return true
              }
              outputChars += parsed.delta.length
              if (outputChars > AI_STREAM_MAX_OUTPUT_CHARS) {
                onError('AI 响应超过 100 万字符上限')
                abort.abort()
                return true
              }
              onDelta(parsed.delta)
              return false
            }
            onError('AI 响应流包含无效事件')
            return true
          } catch {
            onError('AI 响应流包含损坏数据')
            return true
          }
        }
        while (true) {
          const remainingMs = streamDeadline - Date.now()
          if (remainingMs <= 0) throw new Error('AI 响应流总时长超过 2 分钟')
          const timeoutMs = Math.min(AI_STREAM_IDLE_TIMEOUT_MS, remainingMs)
          const timeoutMessage =
            remainingMs <= AI_STREAM_IDLE_TIMEOUT_MS
              ? 'AI 响应流总时长超过 2 分钟'
              : 'AI 响应流等待超过 30 秒'
          const { done, value } = await readAiStreamChunk(
            reader,
            timeoutMs,
            timeoutMessage,
            abort.signal
          )
          if (done) break
          streamBytes += value.byteLength
          if (streamBytes > AI_STREAM_MAX_BYTES) {
            onError('AI 响应流超过 4 MiB 上限')
            abort.abort()
            return
          }
          buf += decoder.decode(value, { stream: true })
          if (buf.length > AI_STREAM_MAX_LINE_CHARS && !buf.includes('\n')) {
            onError('AI 响应流包含超长数据行')
            abort.abort()
            return
          }
          const lines = buf.split('\n')
          buf = lines.pop() ?? ''
          for (const line of lines) {
            if (line.length > AI_STREAM_MAX_LINE_CHARS) {
              onError('AI 响应流包含超长数据行')
              abort.abort()
              return
            }
            if (consumeLine(line)) return
          }
        }
        buf += decoder.decode()
        if (buf.length > AI_STREAM_MAX_LINE_CHARS) {
          onError('AI 响应流包含超长数据行')
          abort.abort()
          return
        }
        if (buf.trim() && consumeLine(buf)) return
        onError('AI 响应流意外中断')
      } catch (e: unknown) {
        if ((e as Error)?.name !== 'AbortError') {
          onError((e as Error)?.message ?? '连接错误')
          abort.abort()
        }
      } finally {
        if (reader) {
          try {
            await reader.cancel()
          } catch {
            // The provider may already have closed the stream.
          }
          reader.releaseLock()
        }
      }
    })()
    return abort
  },

  // @group APIEndpoints > Auth : Auth status — check if password / PIN are configured
  authStatus: async (): Promise<Targeted<AuthStatusPayload>> => {
    const target = captureDaemonTarget()
    const result = await request<unknown>('/auth/status', undefined, target)
    if (!isAuthStatus(result)) throw new Error('服务端返回了无效的认证状态')
    return { ...result, target }
  },

  authSessionStatus: async (
    target: DaemonTarget = captureDaemonTarget()
  ): Promise<Targeted<AuthSessionPayload>> => {
    const result = await request<unknown>('/auth/session', undefined, target)
    if (!isAuthSession(result)) throw new Error('服务端返回了无效的会话状态')
    return { ...result, target }
  },

  // @group APIEndpoints > Auth : First-time password setup
  authSetup: async (password: string): Promise<Targeted<LoginPayload>> => {
    const target = captureDaemonTarget()
    const result = await request<unknown>(
      '/auth/setup',
      { method: 'POST', body: JSON.stringify({ password }) },
      target
    )
    if (!isLoginPayload(result)) throw new Error('服务端返回了无效的登录凭据')
    return { ...result, target }
  },

  // @group APIEndpoints > Auth : Password login
  authLogin: async (password: string): Promise<Targeted<LoginPayload>> => {
    const target = captureDaemonTarget()
    const result = await request<unknown>(
      '/auth/login',
      { method: 'POST', body: JSON.stringify({ password }) },
      target
    )
    if (!isLoginPayload(result)) throw new Error('服务端返回了无效的登录凭据')
    return { ...result, target }
  },

  // @group APIEndpoints > Auth : PIN login (lock screen quick-unlock)
  authPinLogin: async (pin: string): Promise<Targeted<LoginPayload>> => {
    const target = captureDaemonTarget()
    const result = await request<unknown>(
      '/auth/pin/login',
      { method: 'POST', body: JSON.stringify({ pin }) },
      target
    )
    if (!isLoginPayload(result)) throw new Error('服务端返回了无效的登录凭据')
    return { ...result, target }
  },

  // @group APIEndpoints > Auth : Logout — invalidate session
  authLogout: async (): Promise<Targeted<{ success: boolean }>> => {
    const target = captureDaemonTarget()
    const result = await request<{ success: boolean }>(
      '/auth/session',
      { method: 'DELETE' },
      target
    )
    return { ...result, target }
  },

  // @group APIEndpoints > Auth : Change password (requires current password)
  authChangePassword: (
    currentPassword: string,
    newPassword: string
  ): Promise<{ success: boolean }> =>
    request('/auth/change-password', {
      method: 'POST',
      body: JSON.stringify({ current_password: currentPassword, new_password: newPassword }),
    }),

  // @group APIEndpoints > Auth : Disable browser password/PIN/lock settings
  authRemovePassword: async (): Promise<Targeted<{ success: boolean }>> => {
    const target = captureDaemonTarget()
    const result = await request<{ success: boolean }>(
      '/auth/password',
      { method: 'DELETE' },
      target
    )
    return { ...result, target }
  },

  // @group APIEndpoints > Auth : Set or update PIN (4 or 6 digits)
  authSetPin: (pin: string): Promise<{ success: boolean }> =>
    request('/auth/pin', { method: 'POST', body: JSON.stringify({ pin }) }),

  // @group APIEndpoints > Auth : Remove PIN
  authRemovePin: (): Promise<{ success: boolean }> => request('/auth/pin', { method: 'DELETE' }),

  // @group APIEndpoints > Auth : Update auto-lock timeout
  authUpdateLockSettings: (lockTimeoutMins: number | null): Promise<{ success: boolean }> =>
    request('/auth/settings', {
      method: 'PATCH',
      body: JSON.stringify({ lock_timeout_mins: lockTimeoutMins }),
    }),

  // @group APIEndpoints > Telegram : Get Telegram bot config (token is masked)
  getTelegramConfig: (): Promise<TelegramConfigInfo> =>
    validatedRequest('/telegram', isTelegramConfigInfo, '服务端返回了无效的 Telegram 配置'),

  // @group APIEndpoints > Telegram : Update Telegram bot config
  updateTelegramConfig: (cfg: {
    enabled?: boolean
    bot_token?: string
    allowed_chat_ids?: number[]
    notify_on_crash?: boolean
    notify_on_start?: boolean
    notify_on_stop?: boolean
    notify_on_restart?: boolean
  }): Promise<{ success: true; message?: string }> =>
    operationMutation(
      '/telegram',
      { method: 'PUT', body: JSON.stringify(cfg) },
      'Telegram 配置保存'
    ),

  // @group APIEndpoints > Telegram : Send a test message to the first allowed chat
  testTelegram: async (): Promise<{ success: true; message: string }> => {
    const result = await operationMutation('/telegram/test', { method: 'POST' }, 'Telegram 测试')
    if (!result.message) throw new Error('服务端返回了无效的 Telegram 测试结果')
    return { success: true, message: result.message }
  },

  // @group APIEndpoints > Telegram : Validate the bot token and return bot username
  getTelegramBotInfo: (botToken?: string): Promise<TelegramBotInfo> =>
    validatedRequest(
      '/telegram/botinfo',
      isTelegramBotInfo,
      '服务端返回了无效的 Telegram 机器人信息',
      botToken ? { method: 'POST', body: JSON.stringify({ bot_token: botToken }) } : undefined
    ),

  // @group APIEndpoints > Update : Check GitHub for the latest release
  checkUpdate: (): Promise<UpdateInfo> => request('/system/update/check'),

  // @group APIEndpoints > Update : Download and apply the update, then restart daemon
  applyUpdate: (): Promise<{
    success: boolean
    message: string
    version: string
    asset_name: string
  }> => request('/system/update/apply', { method: 'POST', body: JSON.stringify({}) }),

  // @group APIEndpoints > Git : Get git repo info (branch, SHA, dirty, ahead/behind) for a process
  getProcessGit: (id: string, init?: RequestInit): Promise<GitInfo> =>
    validatedRequest(`/processes/${id}/git`, isGitInfo, '服务端返回了无效的 Git 状态', init),

  // @group APIEndpoints > Git : git pull + install deps + restart for a process
  gitPull: (id: string): Promise<GitPullResult> =>
    request(`/processes/${id}/git/pull`, { method: 'POST' }),

  // @group APIEndpoints > Tunnels : List all active tunnels
  getTunnels: async (init?: RequestInit): Promise<{ tunnels: TunnelEntry[] }> => {
    const result = await request<unknown>('/tunnels', init)
    if (!isTunnelListPayload(result)) throw new Error('服务端返回了无效的隧道列表')
    return result
  },

  // @group APIEndpoints > Tunnels : Create a tunnel for a port
  createTunnel: async (body: {
    port: number
    process_name?: string | null
    process_id?: string | null
    provider?: TunnelProvider | null
  }): Promise<{ tunnel?: TunnelEntry; error?: string }> => {
    const result = await request<unknown>('/tunnels', {
      method: 'POST',
      body: JSON.stringify(body),
    })
    if (!isTunnelCreateResult(result)) throw new Error('服务端返回了无效的隧道创建结果')
    return result
  },

  // @group APIEndpoints > Tunnels : Stop a running tunnel (keeps it in the list as stopped)
  stopTunnel: (id: string): Promise<{ success: boolean; error?: string }> =>
    request(`/tunnels/${id}/stop`, { method: 'POST' }),

  // @group APIEndpoints > Tunnels : Remove a tunnel entry from the list entirely (stops first if running)
  removeTunnel: (id: string): Promise<{ success: boolean; error?: string }> =>
    request(`/tunnels/${id}`, { method: 'DELETE' }),

  // @group APIEndpoints > TunnelSettings : Get tunnel provider configuration
  getTunnelSettings: async (): Promise<TunnelSettings> => {
    const result = await request<unknown>('/tunnels/settings')
    if (!isTunnelSettings(result)) throw new Error('服务端返回了无效的隧道设置')
    return result
  },

  // @group APIEndpoints > TunnelSettings : Save tunnel provider configuration
  updateTunnelSettings: (settings: TunnelSettings): Promise<{ success: boolean; error?: string }> =>
    request('/tunnels/settings', { method: 'PUT', body: JSON.stringify(settings) }),

  // @group APIEndpoints > TunnelSettings : Test whether a provider binary is installed
  testTunnelProvider: (provider: TunnelProvider): Promise<{ ok: boolean; message: string }> =>
    request('/tunnels/settings/test', { method: 'POST', body: JSON.stringify({ provider }) }),

  // @group APIEndpoints > TunnelSettings : Install a provider binary via system package manager
  installTunnelProvider: (provider: TunnelProvider): Promise<{ ok: boolean; output: string }> =>
    request('/tunnels/settings/install', { method: 'POST', body: JSON.stringify({ provider }) }),

  // @group APIEndpoints > TunnelSettings : Stream install output as SSE
  streamInstallProvider: async (
    provider: TunnelProvider,
    init?: RequestInit
  ): Promise<EventSource> => {
    const path = '/tunnels/settings/install/stream'
    const target = captureDaemonTarget()
    const query = new URLSearchParams({ provider }).toString()
    const { ticket } = await createStreamTicketForTarget(target, path, query, init)
    const qs = new URLSearchParams(query)
    qs.set('ticket', ticket)
    return new EventSource(`${target.baseUrl}/tunnels/settings/install/stream?${qs}`)
  },

  // @group APIEndpoints > UiSettings : Load persisted UI settings blob from daemon
  getUiSettings: (): Promise<Record<string, unknown>> => request('/system/ui-settings'),

  // @group APIEndpoints > UiSettings : Persist a partial UI settings blob to daemon
  saveUiSettings: (patch: Record<string, unknown>): Promise<void> =>
    request('/system/ui-settings', { method: 'PUT', body: JSON.stringify(patch) }),

  // @group APIEndpoints > TerminalHistory : Load saved command history for a key (e.g. "proc:api-server")
  getTerminalHistory: (key: string): Promise<CmdEntry[]> =>
    request(`/terminals/history/${encodeURIComponent(key)}`),

  // @group APIEndpoints > TerminalHistory : Persist command history for a key
  saveTerminalHistory: (key: string, entries: CmdEntry[]): Promise<void> =>
    request(`/terminals/history/${encodeURIComponent(key)}`, {
      method: 'PUT',
      body: JSON.stringify(entries),
    }),
}

// @group Types > TerminalHistory : Mirrored from Rust CmdEntry
export interface CmdEntry {
  cmd: string
  count: number
}
