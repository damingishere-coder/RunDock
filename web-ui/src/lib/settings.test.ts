import { afterEach, describe, expect, it, vi } from 'vitest'
import { DEFAULT_SETTINGS, loadSettings, saveSettings } from './settings'

afterEach(() => {
  vi.unstubAllGlobals()
  window.localStorage.clear()
})

describe('settings transport', () => {
  it('does not disguise a failed load as defaults', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response('', { status: 503 })))

    await expect(loadSettings()).rejects.toThrow('加载设置失败（HTTP 503）')
  })

  it('does not report a failed save as success', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response('', { status: 500 })))

    await expect(saveSettings(DEFAULT_SETTINGS)).rejects.toThrow('保存设置失败（HTTP 500）')
  })

  it('merges stored values with forward-compatible defaults', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(
        new Response(JSON.stringify({ autoRefresh: false }), {
          status: 200,
          headers: { 'Content-Type': 'application/json' },
        })
      )
    )

    await expect(loadSettings()).resolves.toEqual({ ...DEFAULT_SETTINGS, autoRefresh: false })
  })

  it('rejects a non-object settings payload instead of replacing it with defaults', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(
        new Response(JSON.stringify([]), {
          status: 200,
          headers: { 'Content-Type': 'application/json' },
        })
      )
    )

    await expect(loadSettings()).rejects.toThrow('错误的数据结构')
  })

  it('deep-merges terminal shortcuts and rejects unsafe polling intervals', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(
        new Response(
          JSON.stringify({
            processRefreshInterval: 0,
            healthRefreshInterval: Number.POSITIVE_INFINITY,
            terminalShortcuts: { splitPane: 'ctrl+x' },
          }),
          { status: 200, headers: { 'Content-Type': 'application/json' } }
        )
      )
    )

    const settings = await loadSettings()
    expect(settings.processRefreshInterval).toBe(DEFAULT_SETTINGS.processRefreshInterval)
    expect(settings.healthRefreshInterval).toBe(DEFAULT_SETTINGS.healthRefreshInterval)
    expect(settings.terminalShortcuts).toEqual({
      ...DEFAULT_SETTINGS.terminalShortcuts,
      splitPane: 'ctrl+x',
    })
  })

  it('ignores persisted values whose types do not match the settings schema', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(
        new Response(
          JSON.stringify({
            autoRefresh: 'yes',
            confirmBeforeDelete: null,
            defaultNamespace: ['wrong'],
            logTailLines: -10,
            visibleRowActions: ['logs', 42],
            showQueryDevtools: {},
            terminalShortcuts: { splitPane: 7, duplicateTab: 'alt+x' },
          }),
          { status: 200, headers: { 'Content-Type': 'application/json' } }
        )
      )
    )

    await expect(loadSettings()).resolves.toEqual({
      ...DEFAULT_SETTINGS,
      terminalShortcuts: {
        ...DEFAULT_SETTINGS.terminalShortcuts,
        duplicateTab: 'alt+x',
      },
    })
  })

  it('rejects an oversized settings response before parsing it', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(new Response('x'.repeat(256 * 1_024 + 1), { status: 200 }))
    )

    await expect(loadSettings()).rejects.toThrow('响应正文超过')
  })
})
