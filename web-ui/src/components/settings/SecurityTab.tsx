// @group BusinessLogic > SecurityTab : Security settings — password, PIN, session lock

import { useEffect, useId, useState } from 'react'
import { Check, Eye, EyeOff, Lock, Shield, ShieldOff } from 'lucide-react'
import { api } from '@/lib/api'
import { clearSessionToken, setSessionToken } from '@/lib/auth'
import { Dialog } from '@/components/Dialog'
import { useDialog } from '@/hooks/useDialog'
import { SettingRow } from './shared'
import { card, inputStyle, sectionTitle, selectStyle } from './sharedStyles'

interface PwFieldProps {
  label: string
  value: string
  onChange: (value: string) => void
  autoComplete: string
  show: boolean
  onToggle: () => void
}

function PwField({ label, value, onChange, autoComplete, show, onToggle }: PwFieldProps) {
  const id = useId()
  return (
    <div style={{ marginBottom: 12 }}>
      <label
        htmlFor={id}
        style={{
          display: 'block',
          fontSize: 11,
          fontWeight: 600,
          color: 'var(--color-muted-foreground)',
          marginBottom: 5,
          letterSpacing: '0.04em',
        }}
      >
        {label}
      </label>
      <div style={{ position: 'relative', display: 'flex', alignItems: 'center' }}>
        <input
          id={id}
          type={show ? 'text' : 'password'}
          value={value}
          onChange={event => onChange(event.target.value)}
          autoComplete={autoComplete}
          style={{
            ...inputStyle,
            width: '100%',
            fontSize: 13,
            padding: '8px 36px 8px 12px',
            boxSizing: 'border-box',
            borderRadius: 6,
          }}
        />
        <button
          type="button"
          aria-label={show ? `隐藏${label}` : `显示${label}`}
          onClick={onToggle}
          style={{
            position: 'absolute',
            right: 10,
            background: 'none',
            border: 'none',
            cursor: 'pointer',
            color: 'var(--color-muted-foreground)',
            display: 'flex',
            alignItems: 'center',
            padding: 0,
          }}
        >
          {show ? <EyeOff size={14} /> : <Eye size={14} />}
        </button>
      </div>
    </div>
  )
}

export default function SecurityTab() {
  const { dialogState, danger, handleConfirm, handleCancel } = useDialog()
  // @group BusinessLogic > Security : Change password state
  const [passwordConfigured, setPasswordConfigured] = useState(false)
  const [currentPassword, setCurrentPassword] = useState('')
  const [newPassword, setNewPassword] = useState('')
  const [confirmNewPassword, setConfirmNewPassword] = useState('')
  const [pwChangeError, setPwChangeError] = useState<string | null>(null)
  const [pwChangeSaved, setPwChangeSaved] = useState(false)
  const [pwChangeSaving, setPwChangeSaving] = useState(false)
  const [showCurrentPw, setShowCurrentPw] = useState(false)
  const [showNewPw, setShowNewPw] = useState(false)
  const [showConfirmPw, setShowConfirmPw] = useState(false)

  // @group BusinessLogic > Security : PIN state
  const [pinConfigured, setPinConfigured] = useState(false)
  const [pinInput, setPinInput] = useState('')
  const [pinError, setPinError] = useState<string | null>(null)
  const [pinSaved, setPinSaved] = useState(false)
  const [pinSaving, setPinSaving] = useState(false)

  // @group BusinessLogic > Security : Lock timeout state
  const [lockTimeoutMins, setLockTimeoutMins] = useState<string>('0')
  const [lockSaving, setLockSaving] = useState(false)
  const [lockSaved, setLockSaved] = useState(false)
  const [lockError, setLockError] = useState<string | null>(null)
  const [settingsLoaded, setSettingsLoaded] = useState(false)
  const [settingsLoadError, setSettingsLoadError] = useState<string | null>(null)

  useEffect(() => {
    api
      .authStatus()
      .then(s => {
        setPasswordConfigured(s.password_configured)
        setPinConfigured(s.pin_configured ?? false)
        setLockTimeoutMins(String(s.lock_timeout_mins ?? 0))
        setSettingsLoaded(true)
      })
      .catch(error => {
        setSettingsLoadError(error instanceof Error ? error.message : '读取安全设置失败')
      })
  }, [])

  async function handleSavePassword(e: React.FormEvent) {
    e.preventDefault()
    setPwChangeError(null)
    if (newPassword !== confirmNewPassword) {
      setPwChangeError('两次输入的新密码不一致')
      return
    }
    if (newPassword.length < 8) {
      setPwChangeError('密码至少需要 8 个字符')
      return
    }
    setPwChangeSaving(true)
    try {
      if (passwordConfigured) {
        await api.authChangePassword(currentPassword, newPassword)
      } else {
        const { session_token, target } = await api.authSetup(newPassword)
        setSessionToken(session_token, target)
        setPasswordConfigured(true)
        window.dispatchEvent(new CustomEvent('auth-config-updated'))
      }
      setPwChangeSaved(true)
      setCurrentPassword('')
      setNewPassword('')
      setConfirmNewPassword('')
      setTimeout(() => setPwChangeSaved(false), 2000)
    } catch (err: unknown) {
      setPwChangeError((err as Error)?.message ?? '修改密码失败')
    } finally {
      setPwChangeSaving(false)
    }
  }

  async function handleDisablePassword() {
    const confirmed = await danger(
      '关闭网页密码？',
      '以后打开 RunDock 将直接进入。PIN、通行密钥、自动锁定和现有网页会话也会一起清除。此模式只适合保持 127.0.0.1 本机访问。',
      '关闭密码'
    )
    if (!confirmed) return

    setPwChangeError(null)
    setPwChangeSaving(true)
    try {
      const { target } = await api.authRemovePassword()
      clearSessionToken(target)
      setPasswordConfigured(false)
      setPinConfigured(false)
      setLockTimeoutMins('0')
      setCurrentPassword('')
      setNewPassword('')
      setConfirmNewPassword('')
      window.dispatchEvent(new CustomEvent('auth-config-updated'))
      window.dispatchEvent(new CustomEvent('lock-config-updated'))
    } catch (err: unknown) {
      setPwChangeError((err as Error)?.message ?? '关闭网页密码失败')
    } finally {
      setPwChangeSaving(false)
    }
  }

  async function handleSetPin(e: React.FormEvent) {
    e.preventDefault()
    setPinError(null)
    if (pinInput.length !== 4 && pinInput.length !== 6) {
      setPinError('PIN 必须正好为 4 位或 6 位数字')
      return
    }
    if (!/^\d+$/.test(pinInput)) {
      setPinError('PIN 只能包含数字')
      return
    }
    setPinSaving(true)
    try {
      await api.authSetPin(pinInput)
      setPinConfigured(true)
      setPinSaved(true)
      setPinInput('')
      setTimeout(() => setPinSaved(false), 2000)
    } catch (err: unknown) {
      setPinError((err as Error)?.message ?? '设置 PIN 失败')
    } finally {
      setPinSaving(false)
    }
  }

  async function handleRemovePin() {
    setPinError(null)
    setPinSaving(true)
    try {
      await api.authRemovePin()
      setPinConfigured(false)
      setPinInput('')
    } catch (err: unknown) {
      setPinError((err as Error)?.message ?? '移除 PIN 失败')
    } finally {
      setPinSaving(false)
    }
  }

  async function handleSaveLockTimeout() {
    setLockSaving(true)
    setLockError(null)
    try {
      const mins = lockTimeoutMins === '0' ? null : Number(lockTimeoutMins)
      await api.authUpdateLockSettings(mins)
      setLockSaved(true)
      setTimeout(() => setLockSaved(false), 2000)
      window.dispatchEvent(new CustomEvent('lock-config-updated'))
    } catch (error) {
      setLockError(error instanceof Error ? error.message : '保存自动锁定设置失败')
    } finally {
      setLockSaving(false)
    }
  }

  const strength =
    newPassword.length >= 12 &&
    /[A-Z]/.test(newPassword) &&
    /[0-9]/.test(newPassword) &&
    /[^A-Za-z0-9]/.test(newPassword)
      ? 4
      : newPassword.length >= 10 && /[A-Z]/.test(newPassword) && /[0-9]/.test(newPassword)
        ? 3
        : newPassword.length >= 8
          ? 2
          : 1
  const strengthColors = [
    'var(--color-destructive)',
    'orange',
    '#f0b429',
    'var(--color-status-running)',
  ]

  if (settingsLoadError) {
    return (
      <div role="alert" style={{ color: 'var(--color-destructive)', padding: 16 }}>
        安全设置加载失败，已禁止修改：{settingsLoadError}
      </div>
    )
  }
  if (!settingsLoaded) {
    return <div style={{ padding: 16, color: 'var(--color-muted-foreground)' }}>加载中…</div>
  }

  return (
    <>
      <Dialog
        open={dialogState.open}
        title={dialogState.title}
        message={dialogState.message}
        variant={dialogState.variant}
        confirmLabel={dialogState.confirmLabel}
        cancelLabel={dialogState.cancelLabel}
        onConfirm={handleConfirm}
        onCancel={handleCancel}
      />
      <p style={sectionTitle}>密码</p>
      <div style={card}>
        <div style={{ marginBottom: 4 }}>
          <div
            style={{
              display: 'flex',
              alignItems: 'center',
              gap: 7,
              fontSize: 13,
              fontWeight: 600,
              color: passwordConfigured
                ? 'var(--color-status-running)'
                : 'var(--color-muted-foreground)',
            }}
          >
            {passwordConfigured ? <Shield size={14} /> : <ShieldOff size={14} />}
            {passwordConfigured ? '网页密码已开启' : '网页密码已关闭'}
          </div>
          <div
            style={{
              fontSize: 11,
              color: 'var(--color-muted-foreground)',
              marginTop: 4,
              marginBottom: 16,
            }}
          >
            {passwordConfigured
              ? '访问 RunDock 控制台时需要登录。'
              : '打开 RunDock 将直接进入，不需要输入密码。'}
          </div>
        </div>
        <form onSubmit={handleSavePassword}>
          {passwordConfigured && (
            <>
              <PwField
                label="当前密码"
                value={currentPassword}
                onChange={setCurrentPassword}
                autoComplete="current-password"
                show={showCurrentPw}
                onToggle={() => setShowCurrentPw(p => !p)}
              />
              <div style={{ height: 1, background: 'var(--color-border)', margin: '4px 0 16px' }} />
            </>
          )}
          <PwField
            label={passwordConfigured ? '新密码' : '设置新密码'}
            value={newPassword}
            onChange={setNewPassword}
            autoComplete="new-password"
            show={showNewPw}
            onToggle={() => setShowNewPw(p => !p)}
          />
          <PwField
            label="确认新密码"
            value={confirmNewPassword}
            onChange={setConfirmNewPassword}
            autoComplete="new-password"
            show={showConfirmPw}
            onToggle={() => setShowConfirmPw(p => !p)}
          />

          {newPassword.length > 0 && (
            <div style={{ marginBottom: 14 }}>
              <div style={{ display: 'flex', gap: 4, marginBottom: 4 }}>
                {[1, 2, 3, 4].map(level => (
                  <div
                    key={level}
                    style={{
                      flex: 1,
                      height: 3,
                      borderRadius: 2,
                      background:
                        level <= strength ? strengthColors[strength - 1] : 'var(--color-border)',
                      transition: 'background 0.2s',
                    }}
                  />
                ))}
              </div>
              <span style={{ fontSize: 10, color: 'var(--color-muted-foreground)' }}>
                {newPassword.length < 8
                  ? '太短'
                  : strength === 4
                    ? '强'
                    : strength === 3
                      ? '良好'
                      : '一般 — 请加入大写字母、数字和符号'}
              </span>
            </div>
          )}

          {pwChangeError && (
            <div
              role="alert"
              style={{
                display: 'flex',
                alignItems: 'center',
                gap: 7,
                padding: '8px 12px',
                borderRadius: 6,
                marginBottom: 12,
                background: 'color-mix(in srgb, var(--color-destructive) 10%, transparent)',
                border: '1px solid color-mix(in srgb, var(--color-destructive) 30%, transparent)',
                fontSize: 12,
                color: 'var(--color-destructive)',
              }}
            >
              {pwChangeError}
            </div>
          )}
          {pwChangeSaved && (
            <div
              style={{
                display: 'flex',
                alignItems: 'center',
                gap: 7,
                padding: '8px 12px',
                borderRadius: 6,
                marginBottom: 12,
                background: 'color-mix(in srgb, var(--color-status-running) 10%, transparent)',
                border:
                  '1px solid color-mix(in srgb, var(--color-status-running) 30%, transparent)',
                fontSize: 12,
                color: 'var(--color-status-running)',
              }}
            >
              <Check size={13} /> {passwordConfigured ? '密码保存成功。' : '网页密码已关闭。'}
            </div>
          )}

          <button
            type="submit"
            disabled={
              pwChangeSaving ||
              (passwordConfigured && !currentPassword) ||
              !newPassword ||
              !confirmNewPassword
            }
            style={{
              display: 'flex',
              alignItems: 'center',
              gap: 7,
              padding: '8px 18px',
              fontSize: 13,
              fontWeight: 600,
              background: pwChangeSaved ? 'var(--color-status-running)' : 'var(--color-primary)',
              color: '#fff',
              border: 'none',
              borderRadius: 6,
              cursor: 'pointer',
              opacity:
                pwChangeSaving ||
                (passwordConfigured && !currentPassword) ||
                !newPassword ||
                !confirmNewPassword
                  ? 0.5
                  : 1,
              transition: 'background 0.2s, opacity 0.15s',
            }}
          >
            <Shield size={13} />
            {pwChangeSaving ? '保存中…' : passwordConfigured ? '更新密码' : '开启网页密码'}
          </button>
        </form>
        {passwordConfigured && (
          <div
            style={{ marginTop: 18, paddingTop: 16, borderTop: '1px solid var(--color-border)' }}
          >
            <button
              type="button"
              onClick={handleDisablePassword}
              disabled={pwChangeSaving}
              style={{
                display: 'flex',
                alignItems: 'center',
                gap: 7,
                padding: '7px 13px',
                fontSize: 12,
                fontWeight: 600,
                background: 'transparent',
                color: 'var(--color-destructive)',
                border: '1px solid var(--color-destructive)',
                borderRadius: 6,
                cursor: pwChangeSaving ? 'not-allowed' : 'pointer',
                opacity: pwChangeSaving ? 0.5 : 1,
              }}
            >
              <ShieldOff size={13} /> 关闭网页密码
            </button>
          </div>
        )}
      </div>

      {passwordConfigured && (
        <>
          <p style={sectionTitle}>PIN</p>
          <div style={card}>
            <SettingRow
              label="快速解锁 PIN"
              description={
                pinConfigured
                  ? 'PIN 已设置。输入新的 PIN 进行替换，或将其移除。'
                  : '为锁屏设置 4 位或 6 位 PIN，比输入完整密码更快。'
              }
              isLast
              control={
                <div
                  style={{
                    display: 'flex',
                    flexDirection: 'column',
                    alignItems: 'flex-end',
                    gap: 6,
                  }}
                >
                  {pinError && (
                    <div
                      role="alert"
                      style={{
                        fontSize: 11,
                        color: 'var(--color-destructive)',
                        textAlign: 'right',
                      }}
                    >
                      {pinError}
                    </div>
                  )}
                  <form
                    onSubmit={handleSetPin}
                    style={{ display: 'flex', gap: 6, alignItems: 'center' }}
                  >
                    <input
                      aria-label={pinConfigured ? '新的快速解锁 PIN' : '快速解锁 PIN'}
                      type="text"
                      inputMode="numeric"
                      pattern="[0-9]*"
                      maxLength={6}
                      value={pinInput}
                      onChange={e => setPinInput(e.target.value.replace(/\D/g, '').slice(0, 6))}
                      placeholder={pinConfigured ? '新 PIN（4 位或 6 位）' : 'PIN（4 位或 6 位）'}
                      style={{
                        ...inputStyle,
                        width: 160,
                        fontSize: 12,
                        padding: '5px 10px',
                        letterSpacing: '0.15em',
                        fontFamily: 'monospace',
                      }}
                    />
                    <button
                      type="submit"
                      disabled={pinSaving || pinInput.length < 4}
                      style={{
                        padding: '5px 12px',
                        fontSize: 12,
                        fontWeight: 500,
                        background: pinSaved
                          ? 'var(--color-status-running)'
                          : 'var(--color-primary)',
                        color: '#fff',
                        border: 'none',
                        borderRadius: 5,
                        cursor: 'pointer',
                        opacity: pinSaving || pinInput.length < 4 ? 0.5 : 1,
                        transition: 'background 0.2s',
                      }}
                    >
                      {pinSaved ? '已保存！' : pinConfigured ? '更新' : '设置 PIN'}
                    </button>
                    {pinConfigured && (
                      <button
                        type="button"
                        onClick={handleRemovePin}
                        disabled={pinSaving}
                        style={{
                          padding: '5px 10px',
                          fontSize: 12,
                          background: 'transparent',
                          border: '1px solid var(--color-destructive)',
                          borderRadius: 5,
                          cursor: 'pointer',
                          color: 'var(--color-destructive)',
                          opacity: pinSaving ? 0.5 : 1,
                        }}
                      >
                        移除
                      </button>
                    )}
                  </form>
                </div>
              }
            />
          </div>

          <p style={sectionTitle}>会话</p>
          <div style={card}>
            <SettingRow
              label="无操作后自动锁定"
              description="无操作一段时间后自动锁定控制台。已设置 PIN 时使用 PIN，否则使用密码。"
              isLast
              control={
                <div style={{ display: 'flex', flexDirection: 'column', gap: 5 }}>
                  <div style={{ display: 'flex', gap: 6, alignItems: 'center' }}>
                    <select
                      aria-label="无操作自动锁定时长"
                      value={lockTimeoutMins}
                      onChange={e => setLockTimeoutMins(e.target.value)}
                      style={{ ...selectStyle, minWidth: 120 }}
                    >
                      <option value="0">已禁用</option>
                      <option value="5">5 分钟</option>
                      <option value="15">15 分钟</option>
                      <option value="30">30 分钟</option>
                      <option value="60">1 小时</option>
                    </select>
                    <button
                      onClick={handleSaveLockTimeout}
                      disabled={lockSaving}
                      style={{
                        display: 'flex',
                        alignItems: 'center',
                        gap: 5,
                        padding: '5px 12px',
                        fontSize: 12,
                        fontWeight: 500,
                        background: lockSaved
                          ? 'var(--color-status-running)'
                          : 'var(--color-primary)',
                        color: '#fff',
                        border: 'none',
                        borderRadius: 5,
                        cursor: 'pointer',
                        opacity: lockSaving ? 0.6 : 1,
                        transition: 'background 0.2s',
                      }}
                    >
                      <Lock size={11} />
                      {lockSaved ? '已保存！' : lockSaving ? '保存中…' : '保存'}
                    </button>
                  </div>
                  {lockError && (
                    <span role="alert" style={{ fontSize: 11, color: 'var(--color-destructive)' }}>
                      {lockError}
                    </span>
                  )}
                </div>
              }
            />
          </div>
        </>
      )}
    </>
  )
}
