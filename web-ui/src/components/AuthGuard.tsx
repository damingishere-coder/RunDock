// @group Authentication : Login gate, fail-closed lock configuration, and inactivity lock

import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react'
import LoginPage from '@/pages/LoginPage'
import { api } from '@/lib/api'
import { clearSessionToken, isAuthenticated, isScreenLocked, setScreenLocked } from '@/lib/auth'
import type { DaemonTarget } from '@/lib/transport'

export interface AuthenticatedShellAccess {
  canLock: boolean
  onLock: () => void
}

interface AuthGuardProps {
  children?: (access: AuthenticatedShellAccess) => ReactNode
  recovery?: ReactNode
}

function safelyReadAuthentication(target?: Pick<DaemonTarget, 'tokenKey'>): boolean {
  try {
    return isAuthenticated(target)
  } catch {
    return false
  }
}

function safelyReadScreenLock(): boolean {
  try {
    return isScreenLocked()
  } catch {
    return true
  }
}

export function AuthGuard({ children, recovery }: AuthGuardProps) {
  const [authMode, setAuthMode] = useState<'checking' | 'disabled' | 'required' | 'error'>(
    'checking'
  )
  const [authed, setAuthed] = useState(safelyReadAuthentication)
  const [locked, setLocked] = useState(safelyReadScreenLock)
  const [lockConfig, setLockConfig] = useState<{
    pinConfigured: boolean
    lockTimeoutMins: number | null
  }>({
    pinConfigured: false,
    lockTimeoutMins: null,
  })
  const [lockConfigError, setLockConfigError] = useState(false)
  const [lockConfigLoading, setLockConfigLoading] = useState(true)
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const authRequestVersion = useRef(0)
  const lockRequestVersion = useRef(0)
  const updateLocked = useCallback((nextLocked: boolean) => {
    try {
      setScreenLocked(nextLocked)
    } catch {
      if (!nextLocked) return
    }
    setLocked(nextLocked)
  }, [])
  const lockScreen = useCallback(() => {
    updateLocked(true)
    // authLogout captures the current target and token synchronously before
    // its first await, so local credentials can be cleared immediately while
    // the best-effort server-side revocation continues in the background.
    void api.authLogout().catch(() => undefined)
    try {
      clearSessionToken()
    } catch {
      // The persisted lock marker remains the fail-closed reload boundary.
    }
  }, [updateLocked])

  useEffect(() => {
    let cancelled = false

    function refreshAuthMode() {
      const requestVersion = ++authRequestVersion.current
      Promise.resolve()
        .then(() => api.authStatus())
        .then(async status => {
          if (cancelled || requestVersion !== authRequestVersion.current) return
          if (status.password_configured) {
            let sessionValid = false
            if (safelyReadAuthentication(status.target)) {
              sessionValid = (await api.authSessionStatus(status.target)).valid
            }
            if (cancelled || requestVersion !== authRequestVersion.current) return
            setAuthMode('required')
            setAuthed(sessionValid)
          } else {
            try {
              clearSessionToken(status.target)
            } catch {
              // A damaged server selection is recovered through the global switcher.
            }
            setAuthed(true)
            updateLocked(false)
            setAuthMode('disabled')
          }
        })
        .catch(() => {
          if (!cancelled && requestVersion === authRequestVersion.current) setAuthMode('error')
        })
    }

    refreshAuthMode()
    window.addEventListener('auth-config-updated', refreshAuthMode)
    return () => {
      cancelled = true
      window.removeEventListener('auth-config-updated', refreshAuthMode)
    }
  }, [updateLocked])

  useEffect(() => {
    if (!authed || authMode !== 'required') return
    let cancelled = false

    function fetchConfig() {
      const requestVersion = ++lockRequestVersion.current
      setLockConfigLoading(true)
      Promise.resolve()
        .then(() => api.authStatus())
        .then(status => {
          if (cancelled || requestVersion !== lockRequestVersion.current) return
          setLockConfig({
            pinConfigured: status.pin_configured ?? false,
            lockTimeoutMins: status.lock_timeout_mins ?? null,
          })
          setLockConfigError(false)
          setLockConfigLoading(false)
        })
        .catch(() => {
          if (!cancelled && requestVersion === lockRequestVersion.current) {
            setLockConfigError(true)
            setLockConfigLoading(false)
          }
        })
    }

    fetchConfig()
    window.addEventListener('lock-config-updated', fetchConfig)
    return () => {
      cancelled = true
      window.removeEventListener('lock-config-updated', fetchConfig)
    }
  }, [authed, authMode])

  useEffect(() => {
    if (authMode !== 'required' || !authed || locked || !lockConfig.lockTimeoutMins) return
    const delayMs = lockConfig.lockTimeoutMins * 60 * 1000

    function resetTimer() {
      if (timerRef.current) clearTimeout(timerRef.current)
      timerRef.current = setTimeout(lockScreen, delayMs)
    }
    const events = ['mousemove', 'keydown', 'click', 'scroll', 'touchstart'] as const
    events.forEach(event => window.addEventListener(event, resetTimer, { passive: true }))
    resetTimer()

    return () => {
      if (timerRef.current) clearTimeout(timerRef.current)
      events.forEach(event => window.removeEventListener(event, resetTimer))
    }
  }, [authMode, authed, locked, lockConfig.lockTimeoutMins, lockScreen])

  if (authMode === 'checking') {
    return <AuthStatusMessage message="正在检查访问设置…" />
  }
  if (authMode === 'error') {
    return (
      <>
        <AuthStatusMessage
          message="无法连接到 RunDock 守护进程。请确认服务已启动后刷新页面。"
          showRetry
        />
        {recovery}
      </>
    )
  }
  if (authMode === 'required' && !authed) {
    return (
      <>
        <LoginPage
          subtitle={locked ? '屏幕已锁定' : undefined}
          onAuthenticated={() => {
            setAuthed(true)
            if (locked) updateLocked(false)
          }}
        />
        {!locked && recovery}
      </>
    )
  }
  if (authMode === 'required' && lockConfigLoading) {
    return <AuthStatusMessage message="正在读取自动锁定设置…" />
  }
  if (authMode === 'required' && lockConfigError) {
    return (
      <AuthStatusMessage message="无法读取自动锁定设置。为保护会话，页面已暂停进入。" showRetry />
    )
  }
  if (authMode === 'required' && locked) {
    return <LoginPage onAuthenticated={() => updateLocked(false)} subtitle="屏幕已锁定" />
  }
  return <>{children?.({ canLock: authMode === 'required', onLock: lockScreen })}</>
}

function AuthStatusMessage({
  message,
  showRetry = false,
}: {
  message: string
  showRetry?: boolean
}) {
  return (
    <div
      role={showRetry ? 'alert' : 'status'}
      aria-live={showRetry ? 'assertive' : 'polite'}
      style={{
        minHeight: '100vh',
        display: 'grid',
        placeItems: 'center',
        background: 'var(--color-background)',
        color: 'var(--color-foreground)',
      }}
    >
      <div style={{ textAlign: 'center', fontSize: 13 }}>
        <div>{message}</div>
        {showRetry && (
          <button
            type="button"
            onClick={() => window.location.reload()}
            style={{
              marginTop: 12,
              padding: '6px 14px',
              borderRadius: 6,
              border: '1px solid var(--color-border)',
              background: 'var(--color-secondary)',
              color: 'var(--color-foreground)',
              cursor: 'pointer',
            }}
          >
            重新连接
          </button>
        )}
      </div>
    </div>
  )
}
