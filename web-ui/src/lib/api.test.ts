import { afterEach, describe, expect, it, vi } from 'vitest'
import { api } from './api'
import { setSessionToken } from './auth'

afterEach(() => {
  vi.useRealTimers()
  vi.unstubAllGlobals()
  window.localStorage.clear()
})

describe('API transport', () => {
  it('encodes script names as one path component', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      new Response(
        JSON.stringify({
          name: 'a/b?#',
          path: '',
          content: '',
          language: 'sh',
          interpreter: null,
          prefix_args: [],
        }),
        {
          status: 200,
          headers: { 'Content-Type': 'application/json' },
        }
      )
    )
    vi.stubGlobal('fetch', fetchMock)

    await api.getScript('a/b?#')

    expect(fetchMock).toHaveBeenCalledWith(
      '/api/v1/scripts/a%2Fb%3F%23',
      expect.objectContaining({ headers: expect.any(Headers) })
    )
  })

  it('preserves a bounded plain-text server error', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(new Response('daemon unavailable', { status: 503 }))
    )

    await expect(api.getNotifications()).rejects.toThrow('daemon unavailable')
  })

  it('accepts successful no-content responses', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(null, { status: 204 })))

    await expect(api.saveUiSettings({ viewMode: 'grid' })).resolves.toBeUndefined()
  })

  it('reports an invalid JSON success response clearly', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response('<html>', { status: 200 })))

    await expect(api.getNotifications()).rejects.toThrow('HTTP 200 返回了无效 JSON')
  })

  it('rejects a resolved notification mutation that reports business failure', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(
        new Response(JSON.stringify({ success: false, message: 'persistence rejected' }), {
          status: 200,
        })
      )
    )

    await expect(
      api.updateGlobalNotifications({
        events: {
          on_crash: true,
          on_restart: true,
          on_start: true,
          on_stop: true,
        },
      })
    ).rejects.toThrow('persistence rejected')
  })

  it('rejects business failures for logs, AI settings, and Telegram mutations', async () => {
    const failure = () =>
      new Response(JSON.stringify({ success: false, message: 'durable commit rejected' }), {
        status: 200,
      })
    vi.stubGlobal(
      'fetch',
      vi
        .fn()
        .mockResolvedValueOnce(failure())
        .mockResolvedValueOnce(failure())
        .mockResolvedValueOnce(failure())
    )

    await expect(api.deleteLogs('process-1')).rejects.toThrow('durable commit rejected')
    await expect(api.aiSaveSettings({ enabled: false })).rejects.toThrow('durable commit rejected')
    await expect(api.updateTelegramConfig({ enabled: false })).rejects.toThrow(
      'durable commit rejected'
    )
  })

  it('rejects malformed Device Flow and Telegram payloads', async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(
        new Response(
          JSON.stringify({
            flow_id: 'not-a-uuid',
            poll_token: '0'.repeat(64),
            user_code: 'ABCD-EFGH',
            verification_uri: 'https://example.test/device',
            expires_in: 900,
            interval: 5,
          })
        )
      )
      .mockResolvedValueOnce(new Response(JSON.stringify({ status: 'future', interval: 0 })))
      .mockResolvedValueOnce(
        new Response(
          JSON.stringify({
            enabled: true,
            bot_token_hint: null,
            bot_token_set: true,
            allowed_chat_ids: [0],
          })
        )
      )
      .mockResolvedValueOnce(
        new Response(JSON.stringify({ ok: 'yes', username: null, first_name: null, error: null }))
      )
    vi.stubGlobal('fetch', fetchMock)

    await expect(api.aiAuthStart()).rejects.toThrow('无效的 GitHub 登录信息')
    await expect(api.aiAuthStatus('flow', '0'.repeat(64))).rejects.toThrow('无效的 GitHub 登录状态')
    await expect(api.getTelegramConfig()).rejects.toThrow('无效的 Telegram 配置')
    await expect(api.getTelegramBotInfo()).rejects.toThrow('无效的 Telegram 机器人信息')
  })

  it('binds Device Flow polling to the per-flow credential', async () => {
    const pollToken = 'a'.repeat(64)
    const fetchMock = vi.fn().mockResolvedValue(
      new Response(JSON.stringify({ status: 'pending', interval: 5 }), {
        status: 200,
        headers: { 'Content-Type': 'application/json' },
      })
    )
    vi.stubGlobal('fetch', fetchMock)

    await expect(api.aiAuthStatus('flow-id', pollToken)).resolves.toEqual({
      status: 'pending',
      interval: 5,
    })

    const request = fetchMock.mock.calls[0]?.[1] as RequestInit
    expect(new Headers(request.headers).get('X-RunDock-Device-Token')).toBe(pollToken)
  })

  it('validates a candidate Telegram token without using the persisted-token endpoint', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      new Response(JSON.stringify({ ok: true, username: 'bot', first_name: 'Bot', error: null }), {
        status: 200,
        headers: { 'Content-Type': 'application/json' },
      })
    )
    vi.stubGlobal('fetch', fetchMock)

    await api.getTelegramBotInfo('candidate-token')

    expect(fetchMock).toHaveBeenCalledWith(
      '/api/v1/telegram/botinfo',
      expect.objectContaining({
        method: 'POST',
        body: JSON.stringify({ bot_token: 'candidate-token' }),
      })
    )
  })

  it('updates process notifications through the metadata-only endpoint', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      new Response(JSON.stringify({ id: 'process-1' }), {
        status: 200,
        headers: { 'Content-Type': 'application/json' },
      })
    )
    vi.stubGlobal('fetch', fetchMock)
    const notify = {
      events: {
        on_crash: true,
        on_restart: false,
        on_start: false,
        on_stop: false,
        on_unhealthy: false,
        on_health_recovered: false,
        on_cron_run: false,
        on_cron_fail: false,
      },
    }

    await expect(api.updateProcessNotifications('process-1', notify)).rejects.toThrow(
      '无效的进程详情'
    )

    expect(fetchMock).toHaveBeenCalledWith(
      '/api/v1/processes/process-1/notifications',
      expect.objectContaining({
        method: 'PATCH',
        body: JSON.stringify({ notify }),
      })
    )
  })

  it('keeps a login response bound to the server that received the credentials', async () => {
    localStorage.setItem(
      'alter_servers',
      JSON.stringify([
        {
          id: 'remote-a',
          name: 'Remote A',
          host: 'a.example.com',
          port: 2999,
          protocol: 'https',
          connectionType: 'direct',
        },
        {
          id: 'remote-b',
          name: 'Remote B',
          host: 'b.example.com',
          port: 2999,
          protocol: 'https',
          connectionType: 'direct',
        },
      ])
    )
    localStorage.setItem('alter_active_server', 'remote-a')

    let resolveFetch!: (response: Response) => void
    const fetchMock = vi.fn().mockReturnValue(
      new Promise<Response>(resolve => {
        resolveFetch = resolve
      })
    )
    vi.stubGlobal('fetch', fetchMock)

    const login = api.authLogin('correct-password')
    localStorage.setItem('alter_active_server', 'remote-b')
    resolveFetch(
      new Response(
        JSON.stringify({
          session_token: 'server-a-token-long-enough',
          expires_at: '2026-08-26T00:00:00Z',
        }),
        { status: 200, headers: { 'Content-Type': 'application/json' } }
      )
    )

    const result = await login
    setSessionToken(result.session_token, result.target)

    expect(result.target.serverId).toBe('remote-a')
    expect(localStorage.getItem('alter_session_remote-a')).toBe('server-a-token-long-enough')
    expect(localStorage.getItem('alter_session_remote-b')).toBeNull()
    expect(fetchMock).toHaveBeenCalledWith(
      'https://a.example.com:2999/api/v1/auth/login',
      expect.any(Object)
    )
  })

  it('rejects malformed system statistics', async () => {
    vi.stubGlobal(
      'fetch',
      vi
        .fn()
        .mockResolvedValue(
          new Response(JSON.stringify({ cpu_percent: 20, ram_total_bytes: 0 }), { status: 200 })
        )
    )

    await expect(api.getSystemStats()).rejects.toThrow('无效的系统状态数据')
  })

  it('rejects malformed stream tickets', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(
        new Response(JSON.stringify({ ticket: 'short', expires_at: 'not-a-date' }), {
          status: 200,
        })
      )
    )

    await expect(api.createStreamTicket('/terminals/ws')).rejects.toThrow('无效的流式访问凭据')
  })

  it('rejects malformed successful login payloads before token storage', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(
        new Response(JSON.stringify({ session_token: '', expires_at: 'not-a-date' }), {
          status: 200,
        })
      )
    )

    await expect(api.authLogin('correct-password')).rejects.toThrow('无效的登录凭据')
  })

  it('rejects malformed environment and namespace payloads', async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(new Response(JSON.stringify({ files: [{ name: '.env' }] })))
      .mockResolvedValueOnce(
        new Response(JSON.stringify({ status: 'future', failures: [], persistence: {} }))
      )
    vi.stubGlobal('fetch', fetchMock)

    await expect(api.listEnvPath('C:\\project')).rejects.toThrow('无效的环境文件列表')
    await expect(api.startNamespace('default')).rejects.toThrow('无效的命名空间操作结果')
  })

  it('rejects malformed AI and tunnel response payloads', async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(new Response(JSON.stringify({ enabled: true })))
      .mockResolvedValueOnce(new Response(JSON.stringify({ models: [{ id: 'model-only' }] })))
      .mockResolvedValueOnce(
        new Response(
          JSON.stringify({
            tunnels: [
              {
                id: 'bad-port',
                port: 70_000,
                process_name: null,
                process_id: null,
                provider: 'cloudflare',
                public_url: null,
                status: 'active',
                error: null,
                created_at: '2026-08-26T00:00:00Z',
              },
            ],
          })
        )
      )
    vi.stubGlobal('fetch', fetchMock)

    await expect(api.aiGetSettings()).rejects.toThrow('无效的 AI 设置')
    await expect(api.aiGetModels()).rejects.toThrow('无效的 AI 模型列表')
    await expect(api.getTunnels()).rejects.toThrow('无效的隧道列表')
  })

  it('rejects malformed process, project action, script, and git payloads', async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(new Response(JSON.stringify({ id: 'incomplete-process' })))
      .mockResolvedValueOnce(
        new Response(
          JSON.stringify({
            project_id: 'project-1',
            action: 'start',
            success: true,
            persistence_error: null,
            results: [{ process_id: 'process-1', name: 'app', success: 'yes', error: null }],
          })
        )
      )
      .mockResolvedValueOnce(
        new Response(JSON.stringify({ scripts: [{ name: 'missing-fields' }] }))
      )
      .mockResolvedValueOnce(
        new Response(
          JSON.stringify({
            is_git_repo: true,
            dirty: false,
            ahead: -1,
            behind: 0,
            pkg_manager: 'npm',
          })
        )
      )
    vi.stubGlobal('fetch', fetchMock)

    await expect(api.getProcess('process-1')).rejects.toThrow('无效的进程详情')
    await expect(api.startProject('project-1')).rejects.toThrow('无效的项目操作结果')
    await expect(api.listScripts()).rejects.toThrow('无效的脚本列表')
    await expect(api.getProcessGit('process-1')).rejects.toThrow('无效的 Git 状态')
  })

  it('rejects ambiguous AI stream events', async () => {
    const encoder = new TextEncoder()
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(encoder.encode('data: {}\n'))
        controller.close()
      },
    })
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(stream, { status: 200 })))

    const error = new Promise<string>(resolve => {
      api.aiChat(
        { message: 'hello', history: [] },
        () => undefined,
        () => undefined,
        resolve
      )
    })

    await expect(error).resolves.toBe('AI 响应流包含无效事件')
  })

  it('rejects a complete AI stream line that exceeds the line bound', async () => {
    const encoder = new TextEncoder()
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(encoder.encode(`data: ${'x'.repeat(256 * 1024)}\n`))
        controller.close()
      },
    })
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(stream, { status: 200 })))

    const error = new Promise<string>(resolve => {
      api.aiChat(
        { message: 'hello', history: [] },
        () => undefined,
        () => undefined,
        resolve
      )
    })

    await expect(error).resolves.toBe('AI 响应流包含超长数据行')
  })

  it('times out an AI stream that stops producing data', async () => {
    vi.useFakeTimers()
    const stream = new ReadableStream<Uint8Array>()
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(stream, { status: 200 })))

    const error = new Promise<string>(resolve => {
      api.aiChat(
        { message: 'hello', history: [] },
        () => undefined,
        () => undefined,
        resolve
      )
    })
    await vi.advanceTimersByTimeAsync(30_001)

    await expect(error).resolves.toBe('AI 响应流等待超过 30 秒')
  })
})
