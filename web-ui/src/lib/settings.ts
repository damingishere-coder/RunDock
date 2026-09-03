// @group Configuration : User settings schema — persisted to daemon data directory via REST API

import { daemonFetch, readResponseTextBounded } from '@/lib/transport'

// @group Types : Full settings schema with defaults
export interface AppSettings {
  // Polling
  autoRefresh: boolean
  processRefreshInterval: number // ms — how often to poll /processes
  healthRefreshInterval: number // ms — how often to poll /system/health

  // Behaviour
  confirmBeforeDelete: boolean
  confirmBeforeShutdown: boolean

  // Defaults for new processes/cron jobs
  defaultNamespace: string

  // Log viewer
  logTailLines: number // default lines to fetch in log viewer

  // UI
  visibleRowActions: string[] // which secondary actions show inline in process rows; others go in ⋯

  // Developer
  showQueryDevtools: boolean // show React Query devtools panel (dev mode only)

  // Terminal
  terminalShortcuts: {
    splitPane: string // default: ctrl+shift+t
    duplicateTab: string // default: alt+t
    newTab: string // default: ctrl+t
  }
}

// @group Constants : Default settings values
export const DEFAULT_SETTINGS: AppSettings = {
  autoRefresh: true,
  processRefreshInterval: 3000,
  healthRefreshInterval: 5000,

  confirmBeforeDelete: true,
  confirmBeforeShutdown: true,

  defaultNamespace: 'default',

  logTailLines: 200,

  visibleRowActions: ['logs'],

  showQueryDevtools: false,

  terminalShortcuts: {
    splitPane: 'ctrl+shift+t',
    duplicateTab: 'alt+t',
    newTab: 'ctrl+t',
  },
}

// @group Utilities > API : Base fetch helper for settings (can't import api.ts — circular dep risk)
// @group Utilities > Load : Fetch settings from daemon — merges with defaults for forward-compat
export async function loadSettings(): Promise<AppSettings> {
  const res = await daemonFetch('/system/ui-settings')
  if (!res.ok) throw new Error(`加载设置失败（HTTP ${res.status}）`)
  const body = await readResponseTextBounded(res, 256 * 1_024)
  let payload: unknown
  try {
    payload = JSON.parse(body)
  } catch {
    throw new Error('加载设置失败：服务端返回了无效 JSON')
  }
  if (!isRecord(payload)) throw new Error('加载设置失败：服务端返回了错误的数据结构')
  const raw = payload
  const processRefreshInterval = pollingIntervalOr(
    raw.processRefreshInterval,
    DEFAULT_SETTINGS.processRefreshInterval
  )
  const healthRefreshInterval = pollingIntervalOr(
    raw.healthRefreshInterval,
    DEFAULT_SETTINGS.healthRefreshInterval
  )
  const shortcuts = isRecord(raw.terminalShortcuts) ? raw.terminalShortcuts : {}
  return {
    autoRefresh: booleanOr(raw.autoRefresh, DEFAULT_SETTINGS.autoRefresh),
    processRefreshInterval,
    healthRefreshInterval,
    confirmBeforeDelete: booleanOr(raw.confirmBeforeDelete, DEFAULT_SETTINGS.confirmBeforeDelete),
    confirmBeforeShutdown: booleanOr(
      raw.confirmBeforeShutdown,
      DEFAULT_SETTINGS.confirmBeforeShutdown
    ),
    defaultNamespace: boundedStringOr(raw.defaultNamespace, DEFAULT_SETTINGS.defaultNamespace, 128),
    logTailLines: integerInRangeOr(raw.logTailLines, DEFAULT_SETTINGS.logTailLines, 1, 10_000),
    visibleRowActions: stringArrayOr(raw.visibleRowActions, DEFAULT_SETTINGS.visibleRowActions),
    showQueryDevtools: booleanOr(raw.showQueryDevtools, DEFAULT_SETTINGS.showQueryDevtools),
    terminalShortcuts: {
      splitPane: boundedStringOr(
        shortcuts.splitPane,
        DEFAULT_SETTINGS.terminalShortcuts.splitPane,
        64
      ),
      duplicateTab: boundedStringOr(
        shortcuts.duplicateTab,
        DEFAULT_SETTINGS.terminalShortcuts.duplicateTab,
        64
      ),
      newTab: boundedStringOr(shortcuts.newTab, DEFAULT_SETTINGS.terminalShortcuts.newTab, 64),
    },
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function booleanOr(value: unknown, fallback: boolean): boolean {
  return typeof value === 'boolean' ? value : fallback
}

function boundedStringOr(value: unknown, fallback: string, maxLength: number): string {
  return typeof value === 'string' && value.length <= maxLength ? value : fallback
}

function integerInRangeOr(value: unknown, fallback: number, min: number, max: number): number {
  return Number.isInteger(value) && (value as number) >= min && (value as number) <= max
    ? (value as number)
    : fallback
}

function pollingIntervalOr(value: unknown, fallback: number): number {
  return typeof value === 'number' && Number.isFinite(value) && value >= 250
    ? Math.min(value, 5 * 60_000)
    : fallback
}

function stringArrayOr(value: unknown, fallback: string[]): string[] {
  return Array.isArray(value) && value.every(item => typeof item === 'string' && item.length <= 64)
    ? [...new Set(value)]
    : [...fallback]
}

// @group Utilities > Save : Write settings to daemon data directory
export async function saveSettings(settings: AppSettings): Promise<void> {
  const res = await daemonFetch('/system/ui-settings', {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(settings),
  })
  if (!res.ok) throw new Error(`保存设置失败（HTTP ${res.status}）`)
}

// @group Utilities > Reset : Persist defaults, return them
export async function resetSettings(): Promise<AppSettings> {
  await saveSettings(DEFAULT_SETTINGS)
  return { ...DEFAULT_SETTINGS }
}

// @group Constants : Refresh interval options for the dropdown
export const REFRESH_INTERVAL_OPTIONS: { label: string; value: number }[] = [
  { label: '1 秒', value: 1000 },
  { label: '2 秒', value: 2000 },
  { label: '3 秒', value: 3000 },
  { label: '5 秒', value: 5000 },
  { label: '10 秒', value: 10000 },
  { label: '30 秒', value: 30000 },
  { label: '1 分钟', value: 60000 },
]

// @group Constants : Log tail line count options
export const LOG_TAIL_OPTIONS: { label: string; value: number }[] = [
  { label: '50 行', value: 50 },
  { label: '100 行', value: 100 },
  { label: '200 行', value: 200 },
  { label: '500 行', value: 500 },
  { label: '1000 行', value: 1000 },
]
