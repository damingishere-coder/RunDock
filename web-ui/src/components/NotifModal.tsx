// @group BusinessLogic : Shared notification config modals — per-process and per-namespace

import { useEffect, useRef, useState } from 'react'
import { Bell, Save, Send } from 'lucide-react'
import { api } from '@/lib/api'
import type { NotificationConfig, ProcessInfo } from '@/types'
import { secretInputPlaceholder, secretInputValue } from '@/lib/secrets'

// @group Utilities > NotifDefaults
function defaultNotifConfig(): NotificationConfig {
  return {
    events_override: true,
    events: {
      on_crash: true,
      on_restart: false,
      on_start: false,
      on_stop: false,
      on_unhealthy: true,
      on_health_recovered: true,
      on_cron_run: false,
      on_cron_fail: true,
    },
  }
}

// @group Utilities > Styles : Shared modal button styles
const modalPrimaryBtn: React.CSSProperties = {
  display: 'inline-flex',
  alignItems: 'center',
  gap: 5,
  padding: '5px 12px',
  fontSize: 12,
  fontWeight: 500,
  background: 'var(--color-primary)',
  border: 'none',
  borderRadius: 5,
  cursor: 'pointer',
  color: 'var(--color-primary-foreground)',
}

const modalSecBtn: React.CSSProperties = {
  display: 'inline-flex',
  alignItems: 'center',
  gap: 5,
  padding: '5px 12px',
  fontSize: 12,
  background: 'var(--color-secondary)',
  border: '1px solid var(--color-border)',
  borderRadius: 5,
  cursor: 'pointer',
  color: 'var(--color-foreground)',
}

function useModalFocusTrap(onClose: () => void, busy: boolean) {
  const dialogRef = useRef<HTMLDivElement>(null)
  const previousFocusRef = useRef<HTMLElement | null>(null)

  useEffect(() => {
    previousFocusRef.current = document.activeElement as HTMLElement | null
    const timer = window.setTimeout(() => {
      dialogRef.current
        ?.querySelector<HTMLElement>('button:not([disabled]), input:not([disabled])')
        ?.focus()
    }, 0)
    return () => {
      window.clearTimeout(timer)
      previousFocusRef.current?.focus()
    }
  }, [])

  useEffect(() => {
    const handleKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape' && !busy) {
        event.preventDefault()
        onClose()
        return
      }
      if (event.key !== 'Tab') return
      const focusable = dialogRef.current?.querySelectorAll<HTMLElement>(
        'button:not([disabled]), input:not([disabled])'
      )
      if (!focusable?.length) return
      const first = focusable[0]
      const last = focusable[focusable.length - 1]
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault()
        last.focus()
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault()
        first.focus()
      }
    }
    window.addEventListener('keydown', handleKey)
    return () => window.removeEventListener('keydown', handleKey)
  }, [busy, onClose])

  return dialogRef
}

// @group BusinessLogic > ChannelFields : Shared channel config rows (Webhook / Slack / Teams)
function ChannelFields({
  config,
  setWebhook,
  setSlack,
  setTeams,
  setDiscord,
}: {
  config: NotificationConfig
  setWebhook: (p: Partial<NonNullable<NotificationConfig['webhook']>>) => void
  setSlack: (p: Partial<NonNullable<NotificationConfig['slack']>>) => void
  setTeams: (p: Partial<NonNullable<NotificationConfig['teams']>>) => void
  setDiscord: (p: Partial<NonNullable<NotificationConfig['discord']>>) => void
}) {
  const channels = [
    {
      label: 'Webhook',
      enabled: config.webhook?.enabled ?? false,
      onToggle: (v: boolean) => setWebhook({ enabled: v }),
      fields: [
        {
          label: 'URL',
          type: 'url' as const,
          value: secretInputValue(config.webhook?.url),
          placeholder: secretInputPlaceholder(config.webhook?.url, 'https://hooks.example.com/…'),
          onChange: (v: string) => setWebhook({ url: v }),
        },
      ],
    },
    {
      label: 'Slack',
      enabled: config.slack?.enabled ?? false,
      onToggle: (v: boolean) => setSlack({ enabled: v }),
      fields: [
        {
          label: 'Webhook 地址',
          type: 'url' as const,
          value: secretInputValue(config.slack?.webhook_url),
          placeholder: secretInputPlaceholder(
            config.slack?.webhook_url,
            'https://hooks.slack.com/services/…'
          ),
          onChange: (v: string) => setSlack({ webhook_url: v }),
        },
        {
          label: '频道（可选）',
          type: 'text' as const,
          placeholder: '#alerts',
          value: config.slack?.channel ?? '',
          onChange: (v: string) => setSlack({ channel: v }),
        },
      ],
    },
    {
      label: 'Microsoft Teams',
      enabled: config.teams?.enabled ?? false,
      onToggle: (v: boolean) => setTeams({ enabled: v }),
      fields: [
        {
          label: 'Webhook 地址',
          type: 'url' as const,
          value: secretInputValue(config.teams?.webhook_url),
          placeholder: secretInputPlaceholder(
            config.teams?.webhook_url,
            'https://outlook.office.com/webhook/…'
          ),
          onChange: (v: string) => setTeams({ webhook_url: v }),
        },
      ],
    },
    {
      label: 'Discord',
      enabled: config.discord?.enabled ?? false,
      onToggle: (v: boolean) => setDiscord({ enabled: v }),
      fields: [
        {
          label: 'Webhook 地址',
          type: 'url' as const,
          value: secretInputValue(config.discord?.webhook_url),
          placeholder: secretInputPlaceholder(
            config.discord?.webhook_url,
            'https://discord.com/api/webhooks/…'
          ),
          onChange: (v: string) => setDiscord({ webhook_url: v }),
        },
      ],
    },
  ]

  return (
    <>
      {channels.map(ch => (
        <div
          key={ch.label}
          style={{ border: '1px solid var(--color-border)', borderRadius: 6, padding: '8px 12px' }}
        >
          <label
            style={{
              display: 'flex',
              alignItems: 'center',
              gap: 6,
              fontSize: 11,
              fontWeight: 600,
              cursor: 'pointer',
              color: ch.enabled ? 'var(--color-foreground)' : 'var(--color-muted-foreground)',
            }}
          >
            <input
              type="checkbox"
              checked={ch.enabled}
              onChange={e => ch.onToggle(e.target.checked)}
              style={{ accentColor: 'var(--color-primary)', width: 13, height: 13 }}
            />
            {ch.label}
          </label>
          {ch.enabled && (
            <div style={{ marginTop: 8, display: 'flex', flexDirection: 'column', gap: 6 }}>
              {ch.fields.map(f => (
                <div key={f.label}>
                  <div
                    style={{
                      fontSize: 11,
                      color: 'var(--color-muted-foreground)',
                      marginBottom: 3,
                    }}
                  >
                    {f.label}
                  </div>
                  <input
                    aria-label={`${ch.label} ${f.label}`}
                    style={{
                      width: '100%',
                      boxSizing: 'border-box',
                      padding: '5px 8px',
                      fontSize: 12,
                      background: 'var(--color-secondary)',
                      border: '1px solid var(--color-border)',
                      borderRadius: 4,
                      color: 'var(--color-foreground)',
                      outline: 'none',
                    }}
                    type={f.type}
                    placeholder={f.placeholder}
                    value={f.value}
                    onChange={e => f.onChange(e.target.value)}
                  />
                </div>
              ))}
            </div>
          )}
        </div>
      ))}
    </>
  )
}

// @group BusinessLogic > EventPanels : Process and cron event checkbox panels
function EventPanels({
  config,
  setEvents,
  showCronEvents,
}: {
  config: NotificationConfig
  setEvents: (p: Partial<NotificationConfig['events']>) => void
  showCronEvents: boolean
}) {
  const processEvents = [
    ['on_crash', '崩溃'],
    ['on_restart', '重启'],
    ['on_start', '启动'],
    ['on_stop', '停止'],
    ['on_unhealthy', '健康检查失败'],
    ['on_health_recovered', '健康检查恢复'],
  ] as const
  const cronEventKeys = ['on_cron_run', 'on_cron_fail'] as const

  return (
    <div style={{ display: 'flex', gap: 8 }}>
      <div
        style={{
          flex: 1,
          borderRadius: 6,
          border: '1px solid rgba(99,102,241,0.35)',
          background: 'rgba(99,102,241,0.06)',
          padding: '8px 12px',
        }}
      >
        <div
          style={{
            fontSize: 10,
            fontWeight: 700,
            letterSpacing: '0.1em',
            color: '#818cf8',
            textTransform: 'uppercase',
            marginBottom: 8,
            display: 'flex',
            alignItems: 'center',
            gap: 4,
          }}
        >
          <span>⚙</span> 进程
        </div>
        <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
          {processEvents.map(([key, label]) => (
            <label
              key={key}
              style={{
                display: 'flex',
                alignItems: 'center',
                gap: 5,
                fontSize: 12,
                cursor: 'pointer',
              }}
            >
              <input
                type="checkbox"
                checked={!!config.events[key]}
                onChange={e => setEvents({ [key]: e.target.checked })}
                style={{ accentColor: '#818cf8', width: 13, height: 13 }}
              />
              {label}
            </label>
          ))}
        </div>
      </div>

      {showCronEvents && (
        <div
          style={{
            flex: 1,
            borderRadius: 6,
            border: '1px solid rgba(251,191,36,0.35)',
            background: 'rgba(251,191,36,0.06)',
            padding: '8px 12px',
          }}
        >
          <div
            style={{
              fontSize: 10,
              fontWeight: 700,
              letterSpacing: '0.1em',
              color: '#fbbf24',
              textTransform: 'uppercase',
              marginBottom: 8,
              display: 'flex',
              alignItems: 'center',
              gap: 4,
            }}
          >
            <span>⏰</span> 定时任务
          </div>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
            {cronEventKeys.map(key => (
              <label
                key={key}
                style={{
                  display: 'flex',
                  alignItems: 'center',
                  gap: 5,
                  fontSize: 12,
                  cursor: 'pointer',
                }}
              >
                <input
                  type="checkbox"
                  checked={!!config.events[key]}
                  onChange={e => setEvents({ [key]: e.target.checked })}
                  style={{ accentColor: '#fbbf24', width: 13, height: 13 }}
                />
                {{ run: '运行', fail: '失败' }[key.replace('on_cron_', '') as 'run' | 'fail']}
              </label>
            ))}
          </div>
        </div>
      )}
    </div>
  )
}

// @group BusinessLogic > ProcessNotifModal : Per-process notification config modal
export function ProcessNotifModal({
  process,
  onClose,
}: {
  process: ProcessInfo
  onClose: () => void
}) {
  const [config, setConfig] = useState<NotificationConfig>(process.notify ?? defaultNotifConfig())
  const [saving, setSaving] = useState(false)
  const [testing, setTesting] = useState(false)
  const [saved, setSaved] = useState(false)
  const [error, setError] = useState('')
  const busy = saving || testing
  const dialogRef = useModalFocusTrap(onClose, busy)

  const setEvents = (patch: Partial<NotificationConfig['events']>) =>
    setConfig(c => ({ ...c, events_override: true, events: { ...c.events, ...patch } }))
  const setWebhook = (patch: Partial<NonNullable<NotificationConfig['webhook']>>) =>
    setConfig(c => ({ ...c, webhook: { url: '', enabled: false, ...c.webhook, ...patch } }))
  const setSlack = (patch: Partial<NonNullable<NotificationConfig['slack']>>) =>
    setConfig(c => ({ ...c, slack: { webhook_url: '', enabled: false, ...c.slack, ...patch } }))
  const setTeams = (patch: Partial<NonNullable<NotificationConfig['teams']>>) =>
    setConfig(c => ({ ...c, teams: { webhook_url: '', enabled: false, ...c.teams, ...patch } }))
  const setDiscord = (patch: Partial<NonNullable<NotificationConfig['discord']>>) =>
    setConfig(c => ({ ...c, discord: { webhook_url: '', enabled: false, ...c.discord, ...patch } }))

  async function handleSave() {
    setSaving(true)
    setError('')
    try {
      await api.updateProcessNotifications(process.id, config)
      setSaved(true)
      setTimeout(() => {
        setSaved(false)
        onClose()
      }, 1200)
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setSaving(false)
    }
  }

  async function handleTest() {
    setTesting(true)
    setError('')
    try {
      await api.testNotification(config)
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setTesting(false)
    }
  }

  return (
    <div
      ref={dialogRef}
      onClick={e => {
        if (e.target === e.currentTarget && !busy) onClose()
      }}
      role="dialog"
      aria-modal="true"
      aria-label={`配置 ${process.name} 的通知`}
      style={{
        position: 'fixed',
        inset: 0,
        zIndex: 1100,
        background: 'rgba(0,0,0,0.45)',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
      }}
    >
      <div
        style={{
          background: 'var(--color-card)',
          border: '1px solid var(--color-border)',
          borderRadius: 8,
          width: 460,
          maxWidth: '94vw',
          boxShadow: '0 8px 32px rgba(0,0,0,0.3)',
          display: 'flex',
          flexDirection: 'column',
        }}
      >
        <div
          style={{
            padding: '12px 16px',
            borderBottom: '1px solid var(--color-border)',
            display: 'flex',
            alignItems: 'center',
            gap: 8,
          }}
        >
          <Bell size={14} style={{ color: '#a78bfa' }} />
          <strong style={{ flex: 1, fontSize: 13 }}>通知 — {process.name}</strong>
          <button
            onClick={onClose}
            disabled={busy}
            aria-label="关闭进程通知配置"
            style={{
              background: 'none',
              border: 'none',
              cursor: 'pointer',
              fontSize: 16,
              color: 'var(--color-muted-foreground)',
            }}
          >
            ×
          </button>
        </div>
        <fieldset
          disabled={busy}
          aria-busy={busy}
          style={{
            margin: 0,
            border: 0,
            padding: '14px 16px',
            minWidth: 0,
            display: 'flex',
            flexDirection: 'column',
            gap: 12,
          }}
        >
          <EventPanels config={config} setEvents={setEvents} showCronEvents={!!process.cron} />
          <ChannelFields
            config={config}
            setWebhook={setWebhook}
            setSlack={setSlack}
            setTeams={setTeams}
            setDiscord={setDiscord}
          />
          {error && (
            <div role="alert" style={{ fontSize: 12, color: 'var(--color-destructive)' }}>
              {error}
            </div>
          )}
        </fieldset>
        <div
          style={{
            padding: '10px 16px',
            borderTop: '1px solid var(--color-border)',
            display: 'flex',
            gap: 8,
            alignItems: 'center',
          }}
        >
          <button onClick={handleSave} disabled={busy} style={modalPrimaryBtn}>
            <Save size={12} />
            {saving ? '保存中…' : '保存'}
          </button>
          <button onClick={handleTest} disabled={busy} style={modalSecBtn}>
            <Send size={12} />
            {testing ? '…' : '测试'}
          </button>
          <button onClick={onClose} disabled={busy} style={modalSecBtn}>
            取消
          </button>
          {saved && (
            <span style={{ fontSize: 12, color: 'var(--color-status-running)' }}>✓ 已保存</span>
          )}
        </div>
      </div>
    </div>
  )
}

// @group BusinessLogic > NsNotifModal : Per-namespace notification config modal
export function NsNotifModal({ ns, onClose }: { ns: string; onClose: () => void }) {
  const [config, setConfig] = useState<NotificationConfig>(defaultNotifConfig())
  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)
  const [testing, setTesting] = useState(false)
  const [saved, setSaved] = useState(false)
  const [error, setError] = useState('')
  const [loadError, setLoadError] = useState('')
  const busy = loading || saving || testing
  const dialogRef = useModalFocusTrap(onClose, busy)

  useEffect(() => {
    let cancelled = false
    api
      .getNotifications()
      .then(store => {
        if (cancelled) return
        if (store.namespaces[ns]) setConfig(store.namespaces[ns])
        setLoading(false)
      })
      .catch((loadFailure: unknown) => {
        if (cancelled) return
        setLoadError(
          loadFailure instanceof Error ? loadFailure.message : '命名空间通知配置加载失败'
        )
        setLoading(false)
      })
    return () => {
      cancelled = true
    }
  }, [ns])

  const setEvents = (patch: Partial<NotificationConfig['events']>) =>
    setConfig(c => ({ ...c, events_override: true, events: { ...c.events, ...patch } }))
  const setWebhook = (patch: Partial<NonNullable<NotificationConfig['webhook']>>) =>
    setConfig(c => ({ ...c, webhook: { url: '', enabled: false, ...c.webhook, ...patch } }))
  const setSlack = (patch: Partial<NonNullable<NotificationConfig['slack']>>) =>
    setConfig(c => ({ ...c, slack: { webhook_url: '', enabled: false, ...c.slack, ...patch } }))
  const setTeams = (patch: Partial<NonNullable<NotificationConfig['teams']>>) =>
    setConfig(c => ({ ...c, teams: { webhook_url: '', enabled: false, ...c.teams, ...patch } }))
  const setDiscord = (patch: Partial<NonNullable<NotificationConfig['discord']>>) =>
    setConfig(c => ({ ...c, discord: { webhook_url: '', enabled: false, ...c.discord, ...patch } }))

  async function handleSave() {
    setSaving(true)
    setError('')
    try {
      await api.updateNamespaceNotifications(ns, config)
      setSaved(true)
      setTimeout(() => {
        setSaved(false)
        onClose()
      }, 1200)
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setSaving(false)
    }
  }

  async function handleTest() {
    setTesting(true)
    setError('')
    try {
      await api.testNotification(config)
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setTesting(false)
    }
  }

  return (
    <div
      ref={dialogRef}
      onClick={e => {
        if (e.target === e.currentTarget && !busy) onClose()
      }}
      role="dialog"
      aria-modal="true"
      aria-label={`配置命名空间 ${ns} 的通知`}
      style={{
        position: 'fixed',
        inset: 0,
        zIndex: 1100,
        background: 'rgba(0,0,0,0.45)',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
      }}
    >
      <div
        style={{
          background: 'var(--color-card)',
          border: '1px solid var(--color-border)',
          borderRadius: 8,
          width: 460,
          maxWidth: '94vw',
          boxShadow: '0 8px 32px rgba(0,0,0,0.3)',
        }}
      >
        <div
          style={{
            padding: '12px 16px',
            borderBottom: '1px solid var(--color-border)',
            display: 'flex',
            alignItems: 'center',
            gap: 8,
          }}
        >
          <Bell size={14} style={{ color: '#a78bfa' }} />
          <strong style={{ flex: 1, fontSize: 13 }}>命名空间通知 — {ns}</strong>
          <button
            onClick={onClose}
            disabled={busy}
            aria-label="关闭命名空间通知配置"
            style={{
              background: 'none',
              border: 'none',
              cursor: 'pointer',
              fontSize: 16,
              color: 'var(--color-muted-foreground)',
            }}
          >
            ×
          </button>
        </div>
        {loading ? (
          <div
            style={{
              padding: 24,
              textAlign: 'center',
              color: 'var(--color-muted-foreground)',
              fontSize: 13,
            }}
          >
            加载中…
          </div>
        ) : loadError ? (
          <div
            role="alert"
            style={{ padding: 24, color: 'var(--color-destructive)', fontSize: 13 }}
          >
            配置加载失败，已禁止保存默认值：{loadError}
          </div>
        ) : (
          <>
            <fieldset
              disabled={busy}
              aria-busy={busy}
              style={{
                margin: 0,
                border: 0,
                padding: '14px 16px',
                minWidth: 0,
                display: 'flex',
                flexDirection: 'column',
                gap: 12,
              }}
            >
              <EventPanels config={config} setEvents={setEvents} showCronEvents />
              <ChannelFields
                config={config}
                setWebhook={setWebhook}
                setSlack={setSlack}
                setTeams={setTeams}
                setDiscord={setDiscord}
              />
              {error && (
                <div role="alert" style={{ fontSize: 12, color: 'var(--color-destructive)' }}>
                  {error}
                </div>
              )}
            </fieldset>
            <div
              style={{
                padding: '10px 16px',
                borderTop: '1px solid var(--color-border)',
                display: 'flex',
                gap: 8,
                alignItems: 'center',
              }}
            >
              <button onClick={handleSave} disabled={busy} style={modalPrimaryBtn}>
                <Save size={12} />
                {saving ? '保存中…' : '保存'}
              </button>
              <button onClick={handleTest} disabled={busy} style={modalSecBtn}>
                <Send size={12} />
                {testing ? '…' : '测试'}
              </button>
              <button onClick={onClose} disabled={busy} style={modalSecBtn}>
                取消
              </button>
              {saved && (
                <span style={{ fontSize: 12, color: 'var(--color-status-running)' }}>✓ 已保存</span>
              )}
            </div>
          </>
        )}
      </div>
    </div>
  )
}
