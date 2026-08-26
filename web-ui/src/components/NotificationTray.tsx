// @group BusinessLogic : Notification tray — slide-in activity feed panel

import { useCallback, useEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { X, Settings, CheckCheck, Trash2 } from 'lucide-react'
import { useNavigate } from 'react-router-dom'
import type { AppNotification } from '@/hooks/useNotificationTray'
import { eventConfig, relativeTime } from '@/hooks/useNotificationTray'

// @group BusinessLogic > NotificationTray : Props
interface NotificationTrayProps {
  open: boolean
  notifications: AppNotification[]
  onClose: () => void
  onMarkAllRead: () => void
  onClearAll: () => void
  onDismiss: (id: string) => void
}

// @group Utilities > Styles : Tray style tokens
const trayWidth = 320

const iconBtn: React.CSSProperties = {
  display: 'inline-flex',
  alignItems: 'center',
  justifyContent: 'center',
  width: 28,
  height: 28,
  background: 'transparent',
  border: 'none',
  cursor: 'pointer',
  color: 'var(--color-muted-foreground)',
  borderRadius: 5,
}

const actionBtn: React.CSSProperties = {
  display: 'inline-flex',
  alignItems: 'center',
  gap: 4,
  padding: '4px 8px',
  fontSize: 11,
  fontWeight: 500,
  background: 'transparent',
  border: '1px solid var(--color-border)',
  borderRadius: 4,
  cursor: 'pointer',
  color: 'var(--color-muted-foreground)',
}

// @group BusinessLogic > NotificationTray : Main overlay component
export function NotificationTray({
  open,
  notifications,
  onClose,
  onMarkAllRead,
  onClearAll,
  onDismiss,
}: NotificationTrayProps) {
  const navigate = useNavigate()
  const [now, setNow] = useState(() => Date.now())
  const panelRef = useRef<HTMLDivElement>(null)
  const closeButtonRef = useRef<HTMLButtonElement>(null)
  const previousFocusRef = useRef<HTMLElement | null>(null)

  const closeTray = useCallback(() => {
    const previous = previousFocusRef.current
    onClose()
    if (previous?.isConnected) previous.focus()
  }, [onClose])

  useEffect(() => {
    if (!open) return
    previousFocusRef.current = document.activeElement as HTMLElement | null
    const focusTimer = window.setTimeout(() => closeButtonRef.current?.focus(), 0)
    return () => {
      window.clearTimeout(focusTimer)
      const previous = previousFocusRef.current
      if (previous?.isConnected) previous.focus()
      previousFocusRef.current = null
    }
  }, [open])

  // @group BusinessLogic > Keyboard : Close on Escape
  useEffect(() => {
    if (!open) return
    const handler = (e: KeyboardEvent) => {
      if (e.key === 'Escape') closeTray()
      if (e.key !== 'Tab') return
      const focusable = panelRef.current?.querySelectorAll<HTMLElement>(
        'button:not([disabled]), a[href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])'
      )
      if (!focusable?.length) return
      const first = focusable[0]
      const last = focusable[focusable.length - 1]
      if (e.shiftKey && document.activeElement === first) {
        e.preventDefault()
        last.focus()
      } else if (!e.shiftKey && document.activeElement === last) {
        e.preventDefault()
        first.focus()
      }
    }
    window.addEventListener('keydown', handler)
    return () => window.removeEventListener('keydown', handler)
  }, [closeTray, open])

  useEffect(() => {
    if (!open) return
    const initialRefresh = window.setTimeout(() => setNow(Date.now()), 0)
    const timer = setInterval(() => setNow(Date.now()), 30_000)
    return () => {
      window.clearTimeout(initialRefresh)
      clearInterval(timer)
    }
  }, [open])

  const tray = (
    <>
      {/* Transparent backdrop — click outside to close */}
      {open && (
        <div
          aria-hidden="true"
          onClick={closeTray}
          style={{ position: 'fixed', inset: 0, zIndex: 199 }}
        />
      )}

      {/* Tray panel — slides in from right, same as AI panel */}
      <div
        ref={panelRef}
        role="dialog"
        aria-modal="true"
        aria-label="通知活动"
        aria-hidden={!open}
        inert={!open}
        style={{
          position: 'fixed',
          top: 0,
          right: 0,
          width: trayWidth,
          height: '100vh',
          zIndex: 200,
          background: 'var(--color-card)',
          borderLeft: '1px solid var(--color-border)',
          display: 'flex',
          flexDirection: 'column',
          boxShadow: '-4px 0 24px rgba(0,0,0,0.4)',
          transform: open ? 'translateX(0)' : `translateX(${trayWidth + 4}px)`,
          transition: 'transform 220ms cubic-bezier(0.25, 0.46, 0.45, 0.94)',
          pointerEvents: open ? 'auto' : 'none',
        }}
      >
        {/* Header */}
        <div
          style={{
            padding: '14px 16px',
            borderBottom: '1px solid var(--color-border)',
            display: 'flex',
            alignItems: 'center',
            gap: 8,
          }}
        >
          <span style={{ fontWeight: 600, fontSize: 14, flex: 1 }}>活动</span>
          <button
            onClick={() => {
              navigate('/notifications')
              closeTray()
            }}
            title="通知设置"
            aria-label="打开通知设置"
            style={iconBtn}
          >
            <Settings size={14} />
          </button>
          <button
            ref={closeButtonRef}
            onClick={closeTray}
            title="关闭"
            aria-label="关闭通知活动"
            style={iconBtn}
          >
            <X size={14} />
          </button>
        </div>

        {/* Toolbar — only when there are items */}
        {notifications.length > 0 && (
          <div
            style={{
              padding: '8px 16px',
              borderBottom: '1px solid var(--color-border)',
              display: 'flex',
              gap: 8,
            }}
          >
            <button onClick={onMarkAllRead} style={actionBtn} title="全部标记为已读">
              <CheckCheck size={12} />
              全部已读
            </button>
            <button
              onClick={onClearAll}
              style={{ ...actionBtn, color: 'var(--color-destructive)' }}
              title="清空全部"
            >
              <Trash2 size={12} />
              清空全部
            </button>
          </div>
        )}

        {/* Notification list */}
        <div style={{ flex: 1, overflowY: 'auto' }}>
          {notifications.length === 0 ? (
            <div
              style={{
                display: 'flex',
                flexDirection: 'column',
                alignItems: 'center',
                justifyContent: 'center',
                height: '100%',
                gap: 10,
                color: 'var(--color-muted-foreground)',
              }}
            >
              <span style={{ fontSize: 32, opacity: 0.5 }}>🔔</span>
              <span style={{ fontSize: 13 }}>暂无活动</span>
              <span style={{ fontSize: 11, textAlign: 'center', maxWidth: 200, lineHeight: 1.5 }}>
                进程事件（崩溃、重启、启动、停止）会显示在这里。
              </span>
            </div>
          ) : (
            notifications.map(n => (
              <NotifRow
                key={n.id}
                n={n}
                now={now}
                onNavigate={() => {
                  navigate(`/processes/${n.processId}`)
                  closeTray()
                }}
                onDismiss={() => onDismiss(n.id)}
              />
            ))
          )}
        </div>
      </div>
    </>
  )

  return createPortal(tray, document.body)
}

// @group BusinessLogic > NotifRow : Single notification row
function NotifRow({
  n,
  now,
  onNavigate,
  onDismiss,
}: {
  n: AppNotification
  now: number
  onNavigate: () => void
  onDismiss: () => void
}) {
  const cfg = eventConfig[n.event]

  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'flex-start',
        gap: 10,
        padding: '10px 16px',
        borderBottom: '1px solid var(--color-border)',
        borderLeft: n.read ? '2px solid transparent' : `2px solid ${cfg.color}`,
        background: n.read ? 'transparent' : 'rgba(255,255,255,0.03)',
      }}
      onMouseEnter={e => (e.currentTarget.style.background = 'var(--color-muted)')}
      onMouseLeave={e =>
        (e.currentTarget.style.background = n.read ? 'transparent' : 'rgba(255,255,255,0.03)')
      }
    >
      {/* Status dot */}
      <span style={{ color: cfg.color, fontSize: 10, marginTop: 4, flexShrink: 0 }}>●</span>

      {/* Content */}
      <div
        role="button"
        tabIndex={0}
        onClick={onNavigate}
        onKeyDown={event => {
          if (event.key === 'Enter' || event.key === ' ') {
            event.preventDefault()
            onNavigate()
          }
        }}
        style={{ flex: 1, minWidth: 0, cursor: 'pointer' }}
      >
        <div style={{ display: 'flex', alignItems: 'baseline', gap: 5 }}>
          <span
            style={{
              fontSize: 13,
              fontWeight: 600,
              overflow: 'hidden',
              textOverflow: 'ellipsis',
              whiteSpace: 'nowrap',
              maxWidth: 150,
            }}
          >
            {n.processName}
          </span>
          <span style={{ fontSize: 11, color: cfg.color, fontWeight: 500, flexShrink: 0 }}>
            {cfg.label}
          </span>
        </div>
        <div style={{ fontSize: 11, color: 'var(--color-muted-foreground)', marginTop: 2 }}>
          {n.detail}
        </div>
        <div
          style={{
            fontSize: 10,
            color: 'var(--color-muted-foreground)',
            marginTop: 3,
            opacity: 0.65,
          }}
        >
          {n.namespace !== 'default' && <span style={{ marginRight: 5 }}>[{n.namespace}]</span>}
          {relativeTime(n.timestamp, now)}
        </div>
      </div>

      {/* Dismiss button */}
      <button
        onClick={e => {
          e.stopPropagation()
          onDismiss()
        }}
        title="忽略"
        aria-label={`忽略 ${n.processName} 的通知`}
        style={{ ...iconBtn, opacity: 0.4, flexShrink: 0, marginTop: 0, width: 20, height: 20 }}
        onMouseEnter={e => {
          e.currentTarget.style.opacity = '1'
        }}
        onMouseLeave={e => {
          e.currentTarget.style.opacity = '0.4'
        }}
      >
        <X size={11} />
      </button>
    </div>
  )
}
