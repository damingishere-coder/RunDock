// @group APIEndpoints : Shared daemon transport policy for auth, server selection and 401 handling

import { getActiveServer, serverBaseUrl, serverTokenKey } from '@/lib/servers'

const DAEMON_REQUEST_TIMEOUT_MS = 30_000
const DAEMON_BODY_TIMEOUT_MS = 30_000

export interface DaemonTarget {
  serverId: string
  baseUrl: string
  token: string | null
  tokenKey: string
}

export function captureDaemonTarget(): DaemonTarget {
  const server = getActiveServer()
  const tokenKey = serverTokenKey(server)
  return {
    serverId: server.id,
    baseUrl: serverBaseUrl(server),
    token: localStorage.getItem(tokenKey),
    tokenKey,
  }
}

export function daemonBaseUrl(): string {
  return captureDaemonTarget().baseUrl
}

export async function daemonFetch(
  path: string,
  init?: RequestInit,
  target: DaemonTarget = captureDaemonTarget()
): Promise<Response> {
  const headers = new Headers(init?.headers)
  if (target.token && !headers.has('Authorization')) {
    headers.set('Authorization', `Bearer ${target.token}`)
  }

  const controller = new AbortController()
  const callerSignal = init?.signal
  const abortFromCaller = () => controller.abort(callerSignal?.reason)
  if (callerSignal?.aborted) abortFromCaller()
  else callerSignal?.addEventListener('abort', abortFromCaller, { once: true })
  let timeoutId: ReturnType<typeof setTimeout> | undefined
  let abortListener: (() => void) | undefined
  try {
    const cancellation = new Promise<never>((_, reject) => {
      abortListener = () =>
        reject(
          controller.signal.reason instanceof Error
            ? controller.signal.reason
            : new DOMException('请求已取消', 'AbortError')
        )
      controller.signal.addEventListener('abort', abortListener, { once: true })
    })
    const timeout = new Promise<never>((_, reject) => {
      timeoutId = setTimeout(() => {
        const error = new Error('守护进程请求等待超过 30 秒')
        controller.abort(error)
        reject(error)
      }, DAEMON_REQUEST_TIMEOUT_MS)
    })
    const response = await Promise.race([
      fetch(`${target.baseUrl}${path}`, { ...init, headers, signal: controller.signal }),
      cancellation,
      timeout,
    ])
    const method = (init?.method ?? 'GET').toUpperCase()
    const isCredentialSubmission =
      (method === 'POST' && /^\/auth\/(?:login|pin\/login|setup)$/.test(path)) ||
      (method === 'DELETE' && path === '/auth/session')
    if (response.status === 401 && headers.has('Authorization') && !isCredentialSubmission) {
      localStorage.removeItem(target.tokenKey)
      window.location.reload()
      throw new Error('会话已过期')
    }
    return response
  } finally {
    if (timeoutId) clearTimeout(timeoutId)
    if (abortListener) controller.signal.removeEventListener('abort', abortListener)
    callerSignal?.removeEventListener('abort', abortFromCaller)
  }
}

export async function readResponseTextBounded(
  response: Response,
  maxBytes: number,
  signal?: AbortSignal | null
): Promise<string> {
  if (!Number.isSafeInteger(maxBytes) || maxBytes <= 0) {
    throw new Error('响应大小上限无效')
  }

  const advertisedLength = Number(response.headers.get('Content-Length'))
  if (Number.isFinite(advertisedLength) && advertisedLength > maxBytes) {
    throw new Error(`响应正文超过 ${maxBytes} 字节上限`)
  }

  if (!response.body) return ''

  const reader = response.body.getReader()
  const decoder = new TextDecoder()
  let receivedBytes = 0
  let text = ''
  let complete = false
  const deadline = Date.now() + DAEMON_BODY_TIMEOUT_MS
  try {
    while (true) {
      if (signal?.aborted) {
        throw signal.reason instanceof Error
          ? signal.reason
          : new DOMException('请求已取消', 'AbortError')
      }
      const remainingMs = deadline - Date.now()
      if (remainingMs <= 0) throw new Error('守护进程响应正文等待超过 30 秒')
      let timeoutId: ReturnType<typeof setTimeout> | undefined
      let abortListener: (() => void) | undefined
      const { done, value } = await Promise.race([
        reader.read(),
        new Promise<never>((_, reject) => {
          timeoutId = setTimeout(
            () => reject(new Error('守护进程响应正文等待超过 30 秒')),
            remainingMs
          )
        }),
        new Promise<never>((_, reject) => {
          if (!signal) return
          abortListener = () =>
            reject(
              signal.reason instanceof Error
                ? signal.reason
                : new DOMException('请求已取消', 'AbortError')
            )
          signal.addEventListener('abort', abortListener, { once: true })
        }),
      ]).finally(() => {
        if (timeoutId) clearTimeout(timeoutId)
        if (abortListener) signal?.removeEventListener('abort', abortListener)
      })
      if (done) {
        complete = true
        return text + decoder.decode()
      }
      receivedBytes += value.byteLength
      if (receivedBytes > maxBytes) {
        throw new Error(`响应正文超过 ${maxBytes} 字节上限`)
      }
      text += decoder.decode(value, { stream: true })
    }
  } finally {
    if (!complete) {
      try {
        await reader.cancel()
      } catch {
        // The server may already have closed the response stream.
      }
    }
    reader.releaseLock()
  }
}
