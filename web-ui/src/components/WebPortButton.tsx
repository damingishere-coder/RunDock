import { useEffect, useRef, useState } from 'react'
import { Globe } from 'lucide-react'
import { projectWebTargets } from '@/lib/processWeb'
import type { RemoteServer } from '@/lib/servers'

interface Props {
  ports: number[]
  server: RemoteServer
  showLabel?: boolean
  preferredPort?: number | null
  ariaLabelPrefix?: string
}

export function WebPortButton({
  ports,
  server,
  showLabel = false,
  preferredPort,
  ariaLabelPrefix,
}: Props) {
  const [open, setOpen] = useState(false)
  const ref = useRef<HTMLDivElement>(null)
  const visiblePorts = preferredPort == null ? ports : ports.filter(port => port === preferredPort)
  const targets = projectWebTargets(visiblePorts, server)

  useEffect(() => {
    if (!open) return
    const handler = (event: MouseEvent) => {
      if (ref.current && !ref.current.contains(event.target as Node)) setOpen(false)
    }
    document.addEventListener('mousedown', handler)
    return () => document.removeEventListener('mousedown', handler)
  }, [open])

  if (visiblePorts.length === 0) return null

  if (targets.length === 0) {
    return (
      <button
        type="button"
        disabled
        title="SSH 连接只转发 RunDock 端口，无法直接打开项目端口"
        aria-label="网页（SSH 连接不可直接打开）"
        style={buttonStyle(showLabel, true)}
      >
        <Globe size={13} />
        {showLabel && <span>{preferredPort == null ? '网页' : `打开网页：${preferredPort}`}</span>}
      </button>
    )
  }

  if (targets.length === 1) {
    const target = targets[0]
    return (
      <a
        href={target.url}
        target="_blank"
        rel="noopener noreferrer"
        title={`打开网页 ${target.url}`}
        aria-label={`${ariaLabelPrefix ? `${ariaLabelPrefix}：` : ''}打开网页 ${target.url}`}
        style={buttonStyle(showLabel)}
      >
        <Globe size={13} />
        {showLabel && <span>{preferredPort == null ? '网页' : `打开网页：${preferredPort}`}</span>}
      </a>
    )
  }

  return (
    <div ref={ref} style={{ position: 'relative', flexShrink: 0 }}>
      <button
        type="button"
        title="选择要打开的网页端口"
        aria-label="选择网页端口"
        aria-expanded={open}
        onClick={() => setOpen(value => !value)}
        style={buttonStyle(showLabel)}
      >
        <Globe size={13} />
        {showLabel && <span>网页</span>}
      </button>
      {open && (
        <div role="menu" style={menuStyle}>
          <div
            style={{ padding: '5px 12px', fontSize: 10, color: 'var(--color-muted-foreground)' }}
          >
            选择网页端口
          </div>
          {targets.map(target => (
            <a
              key={target.port}
              role="menuitem"
              href={target.url}
              target="_blank"
              rel="noopener noreferrer"
              onClick={() => setOpen(false)}
              style={menuItemStyle}
              onMouseEnter={event => {
                event.currentTarget.style.background = 'var(--color-accent)'
              }}
              onMouseLeave={event => {
                event.currentTarget.style.background = 'transparent'
              }}
            >
              <Globe size={13} color="#38bdf8" />
              <span style={{ fontWeight: 600 }}>:{target.port}</span>
              <span
                style={{ marginLeft: 'auto', color: 'var(--color-muted-foreground)', fontSize: 10 }}
              >
                打开
              </span>
            </a>
          ))}
        </div>
      )}
    </div>
  )
}

function buttonStyle(showLabel: boolean, disabled = false): React.CSSProperties {
  return {
    height: showLabel ? 28 : 26,
    minWidth: showLabel ? 54 : 26,
    padding: showLabel ? '0 8px' : 0,
    display: 'inline-flex',
    alignItems: 'center',
    justifyContent: 'center',
    gap: 4,
    background: disabled
      ? 'color-mix(in srgb, var(--color-secondary) 60%, transparent)'
      : 'color-mix(in srgb, #38bdf8 10%, var(--color-secondary))',
    border: `1px solid ${disabled ? 'var(--color-border)' : 'color-mix(in srgb, #38bdf8 45%, var(--color-border))'}`,
    borderRadius: 5,
    color: disabled ? 'var(--color-muted-foreground)' : '#38bdf8',
    fontSize: 11,
    fontWeight: 600,
    textDecoration: 'none',
    whiteSpace: 'nowrap',
    cursor: disabled ? 'not-allowed' : 'pointer',
    opacity: disabled ? 0.45 : 1,
    boxSizing: 'border-box',
  }
}

const menuStyle: React.CSSProperties = {
  position: 'absolute',
  top: 'calc(100% + 5px)',
  right: 0,
  background: 'var(--color-card)',
  border: '1px solid var(--color-border)',
  borderRadius: 6,
  padding: '4px 0',
  minWidth: 210,
  zIndex: 510,
  boxShadow: '0 8px 24px rgba(0,0,0,0.42)',
}

const menuItemStyle: React.CSSProperties = {
  display: 'flex',
  alignItems: 'center',
  gap: 8,
  padding: '7px 12px',
  color: 'var(--color-foreground)',
  textDecoration: 'none',
  fontSize: 12,
}
