import { fireEvent, render, screen } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { api } from '@/lib/api'
import { clearSessionToken } from '@/lib/auth'
import { AuthGuard } from './App'

const localTarget = {
  serverId: 'local',
  baseUrl: '/api/v1',
  token: 'session-token',
  tokenKey: 'alter_session_token',
}

vi.mock('@/lib/api', () => ({
  api: {
    authStatus: vi.fn(),
    authSessionStatus: vi.fn(),
    authLogout: vi.fn(),
  },
}))

vi.mock('@/lib/auth', () => ({
  clearSessionToken: vi.fn(),
  getSessionToken: vi.fn(() => 'session-token'),
  isAuthenticated: vi.fn(() => true),
  isScreenLocked: vi.fn(() => localStorage.getItem('alter_screen_locked') === 'true'),
  setScreenLocked: vi.fn((locked: boolean) => {
    if (locked) localStorage.setItem('alter_screen_locked', 'true')
    else localStorage.removeItem('alter_screen_locked')
  }),
}))

describe('AuthGuard lock configuration', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    localStorage.clear()
    vi.mocked(api.authLogout).mockResolvedValue({ success: true, target: localTarget })
  })

  it('fails closed when the authenticated lock configuration cannot be read', async () => {
    vi.mocked(api.authStatus)
      .mockResolvedValueOnce({
        password_configured: true,
        pin_configured: false,
        lock_timeout_mins: 15,
        target: localTarget,
      })
      .mockRejectedValueOnce(new Error('daemon unavailable'))
    vi.mocked(api.authSessionStatus).mockResolvedValue({ valid: true, target: localTarget })

    render(<AuthGuard />)

    expect(
      await screen.findByText('无法读取自动锁定设置。为保护会话，页面已暂停进入。')
    ).toBeVisible()
    expect(screen.getByRole('button', { name: '重新连接' })).toBeVisible()
  })

  it('shows the server recovery entry only when the initial daemon check fails', async () => {
    vi.mocked(api.authStatus).mockRejectedValue(new Error('daemon unavailable'))

    render(
      <AuthGuard recovery={<div>服务器恢复入口</div>}>{() => <div>private shell</div>}</AuthGuard>
    )

    expect(await screen.findByText('服务器恢复入口')).toBeVisible()
    expect(screen.queryByText('private shell')).not.toBeInTheDocument()
  })

  it('keeps a valid session behind the lock screen after a page reload', async () => {
    localStorage.setItem('alter_screen_locked', 'true')
    vi.mocked(api.authStatus).mockResolvedValue({
      password_configured: true,
      pin_configured: true,
      lock_timeout_mins: 15,
      target: localTarget,
    })
    vi.mocked(api.authSessionStatus).mockResolvedValue({ valid: true, target: localTarget })

    render(
      <AuthGuard recovery={<div>server recovery</div>}>{() => <div>private shell</div>}</AuthGuard>
    )

    expect(await screen.findByText('屏幕已锁定')).toBeVisible()
    expect(screen.queryByText('private shell')).not.toBeInTheDocument()
    expect(screen.queryByText('server recovery')).not.toBeInTheDocument()
  })

  it('persists a manual lock before hiding the authenticated shell', async () => {
    vi.mocked(api.authStatus).mockResolvedValue({
      password_configured: true,
      pin_configured: true,
      lock_timeout_mins: 15,
      target: localTarget,
    })
    vi.mocked(api.authSessionStatus).mockResolvedValue({ valid: true, target: localTarget })

    render(
      <AuthGuard>
        {({ onLock }) => (
          <button type="button" onClick={onLock}>
            lock now
          </button>
        )}
      </AuthGuard>
    )

    fireEvent.click(await screen.findByRole('button', { name: 'lock now' }))

    expect(await screen.findByText('屏幕已锁定')).toBeVisible()
    expect(localStorage.getItem('alter_screen_locked')).toBe('true')
    expect(api.authLogout).toHaveBeenCalled()
    expect(clearSessionToken).toHaveBeenCalled()
  })
})
