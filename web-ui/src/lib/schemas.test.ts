import { describe, expect, it } from 'vitest'
import {
  isDaemonHealth,
  isEnvFileContent,
  isEnvFileEntries,
  isGitInfo,
  isLogStatsPayload,
  isMetricsPayload,
  isPortListPayload,
  isProcessInfo,
  isProjectInfo,
  isStreamTicket,
  isSystemStats,
} from './schemas'

describe('daemon response schemas', () => {
  it('rejects partial process objects', () => {
    expect(isProcessInfo({ id: '1', name: 'broken', status: 'running' })).toBe(false)
  })

  it('rejects oversized process and project collections before rendering them', () => {
    const process = {
      id: '1',
      project_id: null,
      name: 'worker',
      script: 'worker.js',
      args: Array.from({ length: 257 }, () => '--flag'),
      cwd: null,
      status: 'stopped',
      pid: null,
      restart_count: 0,
      uptime_secs: null,
      last_exit_code: null,
      autorestart: false,
      max_restarts: 0,
      watch: false,
      namespace: 'default',
      created_at: '2026-08-26T00:00:00Z',
      started_at: null,
      stopped_at: null,
      cron: null,
      cron_next_run: null,
      cron_run_history: [],
      cpu_percent: null,
      memory_bytes: null,
      env: {},
      enabled: true,
    }
    expect(isProcessInfo(process)).toBe(false)
    expect(
      isProjectInfo({
        id: '1',
        kind: 'managed',
        display_name: 'project',
        note: '',
        category: '',
        web_port: null,
        launch_uri: null,
        enabled: true,
        status: 'stopped',
        process_count: 1_001,
        active_process_count: 0,
        cpu_percent: 0,
        memory_bytes: 0,
        members: Array.from({ length: 1_001 }, () => ({
          id: '1',
          name: 'worker',
          status: 'stopped',
          pid: null,
          enabled: true,
        })),
      })
    ).toBe(false)
  })

  it('rejects unknown project states', () => {
    expect(
      isProjectInfo({
        id: '1',
        kind: 'managed',
        display_name: 'broken',
        note: '',
        category: '',
        web_port: null,
        launch_uri: null,
        enabled: true,
        status: 'future-state',
        process_count: 0,
        active_process_count: 0,
        cpu_percent: 0,
        memory_bytes: 0,
        members: [],
      })
    ).toBe(false)
  })

  it('requires the complete daemon health contract', () => {
    expect(isDaemonHealth({ status: 'ok', version: '1.0.0' })).toBe(false)
    expect(
      isDaemonHealth({
        status: 'degraded',
        version: '1.0.0',
        uptime_secs: 10,
        process_count: 2,
        persistence_healthy: false,
        persistence_error: 'disk full',
      })
    ).toBe(true)
  })

  it('accepts bounded system stats and rejects invalid totals', () => {
    expect(
      isSystemStats({
        cpu_percent: 25,
        ram_used_bytes: 512,
        ram_total_bytes: 1024,
        gpu: null,
      })
    ).toBe(true)
    expect(
      isSystemStats({
        cpu_percent: 25,
        ram_used_bytes: 512,
        ram_total_bytes: 0,
        gpu: null,
      })
    ).toBe(false)
  })

  it('requires a bounded stream ticket and a valid expiry timestamp', () => {
    expect(
      isStreamTicket({
        ticket: '0123456789abcdef0123456789abcdef',
        expires_at: '2026-08-26T00:00:00Z',
      })
    ).toBe(true)
    expect(isStreamTicket({ ticket: 'short', expires_at: 'not-a-date' })).toBe(false)
  })

  it('rejects malformed or oversized environment-file payloads', () => {
    expect(isEnvFileEntries([{ name: '.env', path: 'C:\\project\\.env' }])).toBe(true)
    expect(isEnvFileEntries([{ name: '.env' }])).toBe(false)
    expect(isEnvFileContent({ content: 'A=1', exists: true })).toBe(true)
    expect(isEnvFileContent({ content: 42, exists: true })).toBe(false)
  })

  it('bounds operational arrays before pages aggregate or render them', () => {
    expect(
      isPortListPayload({
        ports: [
          {
            port: 2999,
            protocol: 'TCP',
            local_address: '127.0.0.1',
            remote_address: '0.0.0.0',
            state: 'LISTENING',
            pid: 10,
            process_name: 'alter',
            ancestor_pids: [1],
          },
          {
            port: 2999,
            protocol: 'TCP',
            local_address: '127.0.0.1:2999',
            remote_address: '127.0.0.1:50000',
            state: 'TIME_WAIT',
            pid: 0,
            process_name: 'Idle',
            ancestor_pids: [],
          },
        ],
      })
    ).toBe(true)
    expect(
      isMetricsPayload({
        samples: [{ timestamp: '2026-08-26T00:00:00Z', cpu_percent: Number.NaN, memory_bytes: 1 }],
      })
    ).toBe(false)
    expect(
      isLogStatsPayload({
        buckets: Array.from({ length: 513 }, () => ({
          window_start: '2026-08-26T00:00:00Z',
          stdout_count: 0,
          stderr_count: 0,
        })),
      })
    ).toBe(false)
  })

  it('distinguishes unavailable upstream state from a real zero-zero comparison', () => {
    expect(
      isGitInfo({
        is_git_repo: true,
        dirty: false,
        ahead: 0,
        behind: 0,
        upstream_available: false,
        ahead_behind_error: 'no tracking branch',
        pkg_manager: 'cargo',
      })
    ).toBe(true)
    expect(
      isGitInfo({
        is_git_repo: true,
        dirty: false,
        ahead: 0,
        behind: 0,
        pkg_manager: 'cargo',
      })
    ).toBe(false)
  })
})
