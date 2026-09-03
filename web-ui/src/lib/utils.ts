// @group Utilities : Formatting helpers and class name utility

import { clsx, type ClassValue } from 'clsx'
import { twMerge } from 'tailwind-merge'
import type { ProcessInfo, ProcessStatus } from '@/types'

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs))
}

// @group Utilities > Formatting
export function formatUptime(secs: number): string {
  if (secs < 60) return `${secs}秒`
  if (secs < 3600) return `${Math.floor(secs / 60)}分 ${secs % 60}秒`
  if (secs < 86400) return `${Math.floor(secs / 3600)}小时 ${Math.floor((secs % 3600) / 60)}分`
  return `${Math.floor(secs / 86400)}天 ${Math.floor((secs % 86400) / 3600)}小时`
}

export const PROCESS_STATUS_LABELS: Record<ProcessStatus, string> = {
  running: '运行中',
  watching: '监视中',
  stopped: '已停止',
  crashed: '已崩溃',
  errored: '错误',
  starting: '启动中',
  stopping: '停止中',
  sleeping: '休眠中',
}

export function processStatusLabel(status: ProcessStatus): string {
  return PROCESS_STATUS_LABELS[status] ?? status
}

export function formatNextRun(isoStr: string | null): string {
  if (!isoStr) return '-'
  const d = new Date(isoStr)
  const diffMs = d.getTime() - Date.now()
  if (diffMs < 0) return '现在'
  const diffSecs = Math.floor(diffMs / 1000)
  if (diffSecs < 60) return `${diffSecs}秒后`
  if (diffSecs < 3600) return `${Math.floor(diffSecs / 60)}分钟后`
  if (diffSecs < 86400)
    return `${Math.floor(diffSecs / 3600)}小时 ${Math.floor((diffSecs % 3600) / 60)}分钟后`
  return d.toLocaleString('zh-CN', {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  })
}

export function formatLastRun(p: ProcessInfo): string {
  const ts = p.status === 'running' ? p.started_at : (p.stopped_at ?? p.started_at)
  if (!ts) return '-'
  const d = new Date(ts)
  const diffSecs = Math.floor((Date.now() - d.getTime()) / 1000)
  if (diffSecs < 60) return `${diffSecs}秒前`
  if (diffSecs < 3600) return `${Math.floor(diffSecs / 60)}分钟前`
  if (diffSecs < 86400) return `${Math.floor(diffSecs / 3600)}小时前`
  return d.toLocaleDateString('zh-CN', {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  })
}

export const STATUS_COLORS: Record<ProcessStatus, string> = {
  running: 'var(--color-status-running)',
  watching: 'var(--color-status-watching)',
  stopped: 'var(--color-status-stopped)',
  crashed: 'var(--color-status-crashed)',
  errored: 'var(--color-status-errored)',
  starting: 'var(--color-status-starting)',
  stopping: 'var(--color-status-stopping)',
  sleeping: 'var(--color-status-sleeping)',
}

export function statusColor(status: ProcessStatus): string {
  return STATUS_COLORS[status] ?? '#888'
}

export function parseEnvString(raw: string): Record<string, string> {
  const env: Record<string, string> = {}
  for (const pair of raw.split(',')) {
    const idx = pair.indexOf('=')
    if (idx > 0) env[pair.slice(0, idx).trim()] = pair.slice(idx + 1).trim()
  }
  return env
}

// @group Utilities > Env : Parse .env file format (one KEY=VALUE per line, # comments ignored)
export function parseDotEnv(raw: string): Record<string, string> {
  const env: Record<string, string> = {}
  for (const line of raw.split('\n')) {
    const trimmed = line.trim()
    if (!trimmed || trimmed.startsWith('#')) continue
    const idx = trimmed.indexOf('=')
    if (idx > 0) env[trimmed.slice(0, idx).trim()] = trimmed.slice(idx + 1)
  }
  return env
}

// @group Utilities > Env : Serialize env record to .env file format (one KEY=VALUE per line)
export function envToString(env: Record<string, string>): string {
  return Object.entries(env)
    .map(([k, v]) => `${k}=${v}`)
    .join('\n')
}

export function parseArgs(raw: string): string[] {
  return raw.match(/(?:[^\s"']+|"[^"]*"|'[^']*')+/g) ?? []
}

// @group Utilities > Formatting : Format memory bytes into a human-readable string
export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 ** 2) return `${(bytes / 1024).toFixed(1)} KB`
  if (bytes < 1024 ** 3) return `${(bytes / 1024 ** 2).toFixed(1)} MB`
  return `${(bytes / 1024 ** 3).toFixed(2)} GB`
}

// @group Utilities > Formatting : Format CPU percentage with one decimal place
export function formatCpu(pct: number): string {
  return `${pct.toFixed(1)}%`
}
