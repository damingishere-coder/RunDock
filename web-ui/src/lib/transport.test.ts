import { afterEach, describe, expect, it, vi } from 'vitest'
import { daemonFetch, readResponseTextBounded } from './transport'

afterEach(() => {
  vi.unstubAllGlobals()
  window.localStorage.clear()
})

describe('daemon transport authentication', () => {
  it('injects the active server token without leaking a local token to a remote server', async () => {
    localStorage.setItem('alter_session_token', 'local-secret')
    localStorage.setItem(
      'alter_servers',
      JSON.stringify([
        {
          id: 'remote-1',
          name: 'Remote',
          host: 'example.com',
          port: 2999,
          protocol: 'https',
          connectionType: 'direct',
        },
      ])
    )
    localStorage.setItem('alter_active_server', 'remote-1')
    localStorage.setItem('alter_session_remote-1', 'remote-secret')
    const fetchMock = vi.fn().mockResolvedValue(new Response('{}', { status: 200 }))
    vi.stubGlobal('fetch', fetchMock)

    await daemonFetch('/system/health')

    const [url, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(url).toBe('https://example.com:2999/api/v1/system/health')
    expect(new Headers(init.headers).get('Authorization')).toBe('Bearer remote-secret')
  })

  it('clears the active token and rejects a 401 response', async () => {
    localStorage.setItem('alter_session_token', 'expired')
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response('{}', { status: 401 })))

    await expect(daemonFetch('/system/health')).rejects.toThrow('会话已过期')
    expect(localStorage.getItem('alter_session_token')).toBeNull()
  })

  it('preserves login error semantics instead of treating bad credentials as an expired session', async () => {
    localStorage.setItem('alter_session_token', 'stale')
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response('bad password', { status: 401 })))

    const response = await daemonFetch('/auth/login', { method: 'POST' })

    expect(response.status).toBe(401)
    expect(localStorage.getItem('alter_session_token')).toBe('stale')
  })

  it('preserves PIN login 401 semantics for the actual route', async () => {
    localStorage.setItem('alter_session_token', 'stale')
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response('bad PIN', { status: 401 })))

    const response = await daemonFetch('/auth/pin/login', { method: 'POST' })

    expect(response.status).toBe(401)
    expect(localStorage.getItem('alter_session_token')).toBe('stale')
  })

  it('treats GET session validation as an expired session', async () => {
    localStorage.setItem('alter_session_token', 'stale')
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response('{}', { status: 401 })))

    await expect(daemonFetch('/auth/session')).rejects.toThrow('会话已过期')

    expect(localStorage.getItem('alter_session_token')).toBeNull()
  })

  it('preserves logout error semantics for DELETE session', async () => {
    localStorage.setItem('alter_session_token', 'stale')
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(new Response('logout failed', { status: 401 }))
    )

    const response = await daemonFetch('/auth/session', { method: 'DELETE' })

    expect(response.status).toBe(401)
    expect(localStorage.getItem('alter_session_token')).toBe('stale')
  })

  it('rejects a daemon request that never returns headers', async () => {
    vi.useFakeTimers()
    vi.stubGlobal('fetch', vi.fn().mockReturnValue(new Promise<Response>(() => undefined)))

    const assertion = expect(daemonFetch('/system/health')).rejects.toThrow('请求等待超过 30 秒')
    await vi.advanceTimersByTimeAsync(30_001)
    await assertion
    vi.useRealTimers()
  })

  it('cancels response-body consumption when the caller aborts after headers', async () => {
    const cancel = vi.fn()
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(new TextEncoder().encode('{'))
      },
      cancel,
    })
    const controller = new AbortController()
    const pending = readResponseTextBounded(new Response(stream), 1_024, controller.signal)

    controller.abort()

    await expect(pending).rejects.toMatchObject({ name: 'AbortError' })
    expect(cancel).toHaveBeenCalledOnce()
  })
})
