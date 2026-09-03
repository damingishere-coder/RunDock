// @group Authentication : Login page — password and PIN sign-in

import { useCallback, useEffect, useRef, useState } from 'react'
import { KeyRound, Eye, EyeOff } from 'lucide-react'
import { setSessionToken } from '@/lib/auth'
import { api } from '@/lib/api'

interface LoginPageProps {
  onAuthenticated: () => void
  subtitle?: string
}

// @group Authentication > LoginPage : Setup vs login mode
type Mode = 'loading' | 'setup' | 'login'

export default function LoginPage({ onAuthenticated, subtitle }: LoginPageProps) {
  const [mode, setMode] = useState<Mode>('loading')
  const [pinConfigured, setPinConfigured] = useState(false)
  const [usePin, setUsePin] = useState(false)
  const [pin, setPin] = useState('')
  const [password, setPassword] = useState('')
  const [confirmPassword, setConfirmPassword] = useState('')
  const [showPassword, setShowPassword] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)
  const pinRequestRef = useRef(false)

  // @group Authentication > LoginPage : Determine whether password / PIN has been configured
  useEffect(() => {
    api
      .authStatus()
      .then(({ password_configured, pin_configured }) => {
        setMode(password_configured ? 'login' : 'setup')
        setPinConfigured(!!pin_configured)
        setUsePin(!!pin_configured)
      })
      .catch(error => {
        setError(error instanceof Error ? error.message : '无法读取登录配置')
        setMode('login')
      })
  }, [])

  const handlePinDigit = useCallback(
    async (digits: string) => {
      if (digits.length !== 4 && digits.length !== 6) return
      if (pinRequestRef.current) return
      pinRequestRef.current = true
      setLoading(true)
      setError(null)
      try {
        const { session_token, target } = await api.authPinLogin(digits)
        setSessionToken(session_token, target)
        onAuthenticated()
      } catch (pinError: unknown) {
        setError(pinError instanceof Error ? `PIN 登录失败：${pinError.message}` : 'PIN 登录失败')
        setPin('')
      } finally {
        pinRequestRef.current = false
        setLoading(false)
      }
    },
    [onAuthenticated]
  )

  const pressDigit = useCallback(
    (d: string) => {
      if (loading || pin.length >= 6) return
      const next = pin + d
      setPin(next)
      if (next.length === 6) {
        setTimeout(() => handlePinDigit(next), 80)
      }
    },
    [handlePinDigit, loading, pin]
  )

  // @group Authentication > LoginPage : Keyboard input for PIN numpad
  useEffect(() => {
    if (!usePin || mode !== 'login') return
    function handleKey(e: KeyboardEvent) {
      if (loading) return
      if (e.key >= '0' && e.key <= '9') pressDigit(e.key)
      else if (e.key === 'Backspace') setPin(p => p.slice(0, -1))
      else if (e.key === 'Enter' && (pin.length === 4 || pin.length === 6)) {
        void handlePinDigit(pin)
      }
    }
    window.addEventListener('keydown', handleKey)
    return () => window.removeEventListener('keydown', handleKey)
  }, [usePin, mode, loading, pin, pressDigit, handlePinDigit])

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault()
    setError(null)
    setLoading(true)

    try {
      if (mode === 'setup') {
        if (password !== confirmPassword) {
          setError('两次输入的密码不一致')
          return
        }
        if (password.length < 8) {
          setError('密码至少需要 8 个字符')
          return
        }
        const { session_token, target } = await api.authSetup(password)
        setSessionToken(session_token, target)
        onAuthenticated()
      } else {
        const { session_token, target } = await api.authLogin(password)
        setSessionToken(session_token, target)
        onAuthenticated()
      }
    } catch (e: unknown) {
      setError((e as Error)?.message ?? '认证失败')
    } finally {
      setLoading(false)
    }
  }

  if (mode === 'loading') {
    return (
      <div style={containerStyle}>
        <div style={cardStyle}>
          <p style={{ color: 'var(--color-muted-foreground)', fontSize: 14 }}>加载中…</p>
        </div>
      </div>
    )
  }

  const logo = (
    <div style={{ textAlign: 'center', marginBottom: 24 }}>
      <img
        src="/rundock-icon.svg"
        alt=""
        style={{
          width: 68,
          height: 68,
          display: 'block',
          margin: '0 auto 10px',
          filter: 'drop-shadow(0 12px 20px rgba(20,123,255,0.2))',
        }}
      />
      <span style={{ fontWeight: 760, fontSize: 28, letterSpacing: '-1.2px', color: '#102a4d' }}>
        Run
      </span>
      <span
        style={{
          fontWeight: 760,
          fontSize: 28,
          letterSpacing: '-1.2px',
          color: 'var(--color-primary)',
        }}
      >
        Dock
      </span>
      <p style={{ margin: '8px 0 0', fontSize: 13, color: 'var(--color-muted-foreground)' }}>
        {subtitle ?? (mode === 'setup' ? '设置密码以保护你的控制台' : '登录以继续')}
      </p>
    </div>
  )

  const errorBanner = error && (
    <div
      role="alert"
      aria-live="assertive"
      style={{
        background: 'color-mix(in srgb, var(--color-destructive) 15%, transparent)',
        border: '1px solid var(--color-destructive)',
        borderRadius: 6,
        padding: '8px 12px',
        fontSize: 13,
        color: 'var(--color-destructive)',
        marginBottom: 16,
      }}
    >
      {error}
    </div>
  )

  // @group Authentication > LoginPage : PIN numpad view
  if (mode === 'login' && usePin) {
    return (
      <div style={containerStyle}>
        <div style={{ ...cardStyle, textAlign: 'center' }}>
          {logo}
          {errorBanner}

          {/* PIN dots */}
          <div style={{ display: 'flex', justifyContent: 'center', gap: 12, marginBottom: 20 }}>
            {[0, 1, 2, 3, 4, 5].map(i => (
              <div
                key={i}
                style={{
                  width: 12,
                  height: 12,
                  borderRadius: '50%',
                  background: i < pin.length ? 'var(--color-primary)' : 'var(--color-border)',
                  transition: 'background 0.15s',
                  display: pin.length <= 4 && i >= 4 ? 'none' : 'block',
                }}
              />
            ))}
          </div>

          {/* Numpad */}
          <div
            style={{
              display: 'grid',
              gridTemplateColumns: 'repeat(3, 1fr)',
              gap: 10,
              maxWidth: 220,
              margin: '0 auto 20px',
            }}
          >
            {['1', '2', '3', '4', '5', '6', '7', '8', '9', '', '0', '⌫'].map((d, idx) =>
              d === '' ? (
                <div key={idx} />
              ) : (
                <button
                  key={idx}
                  onClick={() => (d === '⌫' ? setPin(p => p.slice(0, -1)) : pressDigit(d))}
                  disabled={loading}
                  style={{
                    width: 64,
                    height: 64,
                    borderRadius: 32,
                    fontSize: d === '⌫' ? 20 : 22,
                    fontWeight: 500,
                    background: 'var(--color-card)',
                    border: '1px solid var(--color-border)',
                    cursor: 'pointer',
                    color: 'var(--color-foreground)',
                    opacity: loading ? 0.5 : 1,
                  }}
                >
                  {d}
                </button>
              )
            )}
          </div>

          <button
            type="button"
            onClick={() => void handlePinDigit(pin)}
            disabled={loading || (pin.length !== 4 && pin.length !== 6)}
            style={{ ...submitBtnStyle, width: 220, margin: '0 auto 16px' }}
          >
            {loading ? '验证中…' : '确认 PIN'}
          </button>

          {/* Switch to password */}
          <button
            onClick={() => {
              setUsePin(false)
              setError(null)
            }}
            style={{
              background: 'none',
              border: 'none',
              cursor: 'pointer',
              fontSize: 12,
              color: 'var(--color-muted-foreground)',
              textDecoration: 'underline',
            }}
          >
            改用密码
          </button>
        </div>
      </div>
    )
  }

  return (
    <div style={containerStyle}>
      <div style={cardStyle}>
        {logo}
        {errorBanner}

        {/* Password form */}
        <form onSubmit={handleSubmit} style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
          <div>
            <label htmlFor="login-password" style={labelStyle}>
              密码
            </label>
            <div style={{ position: 'relative' }}>
              <input
                id="login-password"
                type={showPassword ? 'text' : 'password'}
                value={password}
                onChange={e => setPassword(e.target.value)}
                placeholder="请输入密码"
                autoFocus
                required
                style={inputStyle}
              />
              <button
                type="button"
                aria-label={showPassword ? '隐藏密码' : '显示密码'}
                onClick={() => setShowPassword(v => !v)}
                style={eyeBtnStyle}
                tabIndex={-1}
              >
                {showPassword ? <EyeOff size={14} /> : <Eye size={14} />}
              </button>
            </div>
          </div>

          {mode === 'setup' && (
            <div>
              <label htmlFor="login-confirm-password" style={labelStyle}>
                确认密码
              </label>
              <div style={{ position: 'relative' }}>
                <input
                  id="login-confirm-password"
                  type={showPassword ? 'text' : 'password'}
                  value={confirmPassword}
                  onChange={e => setConfirmPassword(e.target.value)}
                  placeholder="请再次输入密码"
                  required
                  style={inputStyle}
                />
              </div>
            </div>
          )}

          <button type="submit" disabled={loading || !password} style={submitBtnStyle}>
            <KeyRound size={14} />
            {loading ? '请稍候…' : mode === 'setup' ? '设置密码并登录' : '登录'}
          </button>
        </form>

        {/* Switch to PIN */}
        {mode === 'login' && pinConfigured && (
          <div style={{ marginTop: 14, textAlign: 'center' }}>
            <button
              onClick={() => {
                setUsePin(true)
                setError(null)
                setPin('')
              }}
              style={{
                background: 'none',
                border: 'none',
                cursor: 'pointer',
                fontSize: 12,
                color: 'var(--color-muted-foreground)',
                textDecoration: 'underline',
              }}
            >
              改用 PIN
            </button>
          </div>
        )}

        {!subtitle && (
          <p
            style={{
              marginTop: 12,
              fontSize: 11,
              color: 'var(--color-muted-foreground)',
              textAlign: 'center',
              lineHeight: 1.5,
            }}
          >
            {mode === 'setup'
              ? '此密码用于保护 RunDock 控制台。CLI 会通过本地 Token 自动完成认证。'
              : '你也可以使用 CLI，它会自动完成认证。'}
          </p>
        )}
      </div>
    </div>
  )
}

// @group Styles : Login page layout and card styles
const containerStyle: React.CSSProperties = {
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'center',
  height: '100vh',
  padding: 20,
  background:
    'radial-gradient(circle at 50% 0%, rgba(20,123,255,0.18), transparent 38%), linear-gradient(180deg, #fbfdff, var(--color-background))',
}

const cardStyle: React.CSSProperties = {
  width: 360,
  background: 'var(--color-card)',
  border: '1px solid var(--color-border)',
  borderRadius: 22,
  padding: 28,
  boxShadow: '0 24px 70px rgba(31,72,126,0.14)',
}

const labelStyle: React.CSSProperties = {
  display: 'block',
  fontSize: 12,
  fontWeight: 500,
  color: 'var(--color-foreground)',
  marginBottom: 4,
}

const inputStyle: React.CSSProperties = {
  width: '100%',
  padding: '8px 36px 8px 10px',
  fontSize: 13,
  borderRadius: 6,
  border: '1px solid var(--color-border)',
  background: 'var(--color-background)',
  color: 'var(--color-foreground)',
  outline: 'none',
  boxSizing: 'border-box',
}

const eyeBtnStyle: React.CSSProperties = {
  position: 'absolute',
  right: 8,
  top: '50%',
  transform: 'translateY(-50%)',
  background: 'none',
  border: 'none',
  cursor: 'pointer',
  color: 'var(--color-muted-foreground)',
  padding: 0,
  display: 'flex',
}

const submitBtnStyle: React.CSSProperties = {
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'center',
  gap: 8,
  padding: '9px 16px',
  fontSize: 13,
  fontWeight: 600,
  background: 'var(--color-primary)',
  color: 'var(--color-primary-foreground)',
  border: 'none',
  borderRadius: 6,
  cursor: 'pointer',
  opacity: 1,
}
