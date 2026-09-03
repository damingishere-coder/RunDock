// @group BusinessLogic : Sidebar navigation and project controls

import { Plus, RotateCcw, Square, type LucideIcon } from 'lucide-react'
import { useMemo, useState } from 'react'
import { Link, Navigate, useParams } from 'react-router-dom'
import { projectStatusColor, projectStatusLabel } from '@/lib/projects'
import type { ProcessInfo, ProjectInfo } from '@/types'

// @group BusinessLogic > CronJobSubmenu : Namespace list for cron jobs — same pattern as NamespaceSubmenu
export function CronJobSubmenu({
  processes,
  currentNamespace,
  open,
}: {
  processes: ProcessInfo[]
  currentNamespace: string | null
  open: boolean
}) {
  const [filter, setFilter] = useState('')

  const cronJobs = useMemo(() => processes.filter(p => p.cron), [processes])

  const namespaces = useMemo(
    () =>
      [...new Set(cronJobs.map(p => p.namespace || 'default'))].sort((left, right) =>
        left.localeCompare(right)
      ),
    [cronJobs]
  )

  const filtered = filter
    ? namespaces.filter(ns => ns.toLowerCase().includes(filter.toLowerCase()))
    : namespaces

  if (!open || namespaces.length === 0) return null

  return (
    <div style={{ paddingBottom: 2 }}>
      {namespaces.length > 4 && (
        <div style={{ padding: '3px 10px 3px 34px' }}>
          <input
            aria-label="筛选定时任务命名空间"
            value={filter}
            onChange={e => setFilter(e.target.value)}
            placeholder="筛选命名空间…"
            style={{
              width: '100%',
              padding: '3px 8px',
              fontSize: 11,
              borderRadius: 4,
              border: '1px solid var(--color-border)',
              background: 'var(--color-secondary)',
              color: 'var(--color-foreground)',
              outline: 'none',
              boxSizing: 'border-box',
            }}
          />
        </div>
      )}
      {filtered.map(ns => {
        const count = cronJobs.filter(p => (p.namespace || 'default') === ns).length
        const isActive = currentNamespace === ns
        return (
          <Link
            key={ns}
            to={`/cron-jobs?namespace=${encodeURIComponent(ns)}`}
            style={{
              display: 'flex',
              alignItems: 'center',
              gap: 6,
              padding: '4px 14px 4px 34px',
              fontSize: 12,
              color: isActive ? 'var(--color-primary)' : 'var(--color-muted-foreground)',
              textDecoration: 'none',
              fontWeight: isActive ? 600 : 400,
              background: isActive ? 'var(--color-accent)' : 'transparent',
              borderLeft: isActive ? '2px solid var(--color-primary)' : '2px solid transparent',
            }}
            onMouseEnter={e => {
              if (!isActive) e.currentTarget.style.background = 'var(--color-accent)'
            }}
            onMouseLeave={e => {
              if (!isActive) e.currentTarget.style.background = 'transparent'
            }}
          >
            <span
              style={{
                overflow: 'hidden',
                textOverflow: 'ellipsis',
                whiteSpace: 'nowrap',
                flex: 1,
              }}
            >
              {ns === 'default' ? '未分类' : ns}
            </span>
            <span
              style={{
                fontSize: 10,
                flexShrink: 0,
                background: 'var(--color-secondary)',
                border: '1px solid var(--color-border)',
                borderRadius: 3,
                padding: '0 4px',
                opacity: 0.75,
              }}
            >
              {count}
            </span>
          </Link>
        )
      })}
    </div>
  )
}

export function LegacyNamespaceRedirect() {
  const { name } = useParams<{ name: string }>()
  return <Navigate to={`/cron-jobs?namespace=${encodeURIComponent(name ?? 'default')}`} replace />
}

// @group BusinessLogic > NavRowWithAdd : Sidebar nav link with optional ▼ submenu toggle and inline + button
export function NavRowWithAdd({
  to,
  icon: Icon,
  label,
  active,
  onAdd,
  addTitle,
  onToggleNs,
  nsOpen,
}: {
  to: string
  icon: LucideIcon
  label: string
  active: boolean
  onAdd: () => void
  addTitle: string
  onToggleNs?: () => void
  nsOpen?: boolean
}) {
  return (
    <div style={{ display: 'flex', alignItems: 'center' }}>
      <Link
        to={to}
        aria-current={active ? 'page' : undefined}
        style={{
          flex: 1,
          display: 'flex',
          alignItems: 'center',
          gap: 9,
          padding: '7px 16px',
          fontSize: 13,
          color: active ? 'var(--color-primary)' : 'var(--color-foreground)',
          textDecoration: 'none',
          fontWeight: active ? 600 : 500,
          background: active ? 'var(--color-accent)' : 'transparent',
          borderLeft: active ? '2px solid var(--color-primary)' : '2px solid transparent',
        }}
        onMouseEnter={e => {
          if (!active) e.currentTarget.style.background = 'var(--color-accent)'
        }}
        onMouseLeave={e => {
          if (!active) e.currentTarget.style.background = 'transparent'
        }}
      >
        <Icon size={14} />
        {label}
      </Link>
      {onToggleNs && (
        <button
          type="button"
          onClick={onToggleNs}
          title={nsOpen ? '收起列表' : '展开列表'}
          aria-label={`${nsOpen ? '收起' : '展开'}${label}命名空间`}
          aria-expanded={nsOpen}
          style={{
            width: 24,
            height: 32,
            padding: 0,
            display: 'inline-flex',
            alignItems: 'center',
            justifyContent: 'center',
            background: 'transparent',
            border: 'none',
            color: 'var(--color-muted-foreground)',
            cursor: 'pointer',
            fontSize: 9,
            opacity: 0.7,
            transform: nsOpen ? 'rotate(0deg)' : 'rotate(-90deg)',
            transition: 'transform 0.15s',
            flexShrink: 0,
          }}
        >
          ▼
        </button>
      )}
      {/* Inline + button */}
      <button
        type="button"
        onClick={onAdd}
        title={addTitle}
        style={{
          width: 28,
          flexShrink: 0,
          height: 32,
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          background: 'transparent',
          border: 'none',
          cursor: 'pointer',
          color: 'var(--color-muted-foreground)',
          paddingRight: 8,
        }}
        onMouseEnter={e => {
          e.currentTarget.style.color = 'var(--color-primary)'
        }}
        onMouseLeave={e => {
          e.currentTarget.style.color = 'var(--color-muted-foreground)'
        }}
      >
        <Plus size={13} strokeWidth={2} />
      </button>
    </div>
  )
}

// @group BusinessLogic > NavBtn : Sidebar navigation link with active highlight
export function NavBtn({
  to,
  icon: Icon,
  label,
  active,
}: {
  to: string
  icon: LucideIcon
  label: string
  active: boolean
}) {
  return (
    <Link
      to={to}
      aria-current={active ? 'page' : undefined}
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: 9,
        padding: '7px 16px',
        fontSize: 13,
        color: active ? 'var(--color-primary)' : 'var(--color-foreground)',
        textDecoration: 'none',
        fontWeight: active ? 600 : 500,
        background: active ? 'var(--color-accent)' : 'transparent',
        borderLeft: active ? '2px solid var(--color-primary)' : '2px solid transparent',
      }}
      onMouseEnter={e => {
        if (!active) e.currentTarget.style.background = 'var(--color-accent)'
      }}
      onMouseLeave={e => {
        if (!active) e.currentTarget.style.background = 'transparent'
      }}
    >
      <Icon size={14} />
      {label}
    </Link>
  )
}

// @group BusinessLogic > SidebarAction : Text-labelled sidebar action for non-route tools
export function SidebarAction({
  icon: Icon,
  label,
  active,
  badge,
  onClick,
}: {
  icon: LucideIcon
  label: string
  active: boolean
  badge?: number | string
  onClick: () => void
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-pressed={active}
      style={{
        width: '100%',
        display: 'flex',
        alignItems: 'center',
        gap: 9,
        padding: '7px 16px',
        fontSize: 13,
        color: active ? 'var(--color-primary)' : 'var(--color-foreground)',
        fontWeight: active ? 600 : 500,
        background: active ? 'var(--color-accent)' : 'transparent',
        border: 'none',
        borderLeft: active ? '2px solid var(--color-primary)' : '2px solid transparent',
        cursor: 'pointer',
        textAlign: 'left',
        fontFamily: 'inherit',
      }}
    >
      <Icon size={14} />
      <span style={{ flex: 1 }}>{label}</span>
      {badge !== undefined && (
        <span
          aria-label={`${badge} 个已打开项`}
          style={{
            minWidth: 18,
            padding: '0 5px',
            borderRadius: 8,
            background: 'var(--color-secondary)',
            border: '1px solid var(--color-border)',
            color: 'var(--color-muted-foreground)',
            fontSize: 10,
            textAlign: 'center',
          }}
        >
          {badge}
        </span>
      )}
    </button>
  )
}

// @group BusinessLogic > SidebarProjectGroup : Category header with project-level rows only
export function SidebarProjectGroup({
  category,
  projects,
  collapsed,
  onToggle,
  onNavigate,
  onStop,
  onRestart,
  onError,
}: {
  category: string
  projects: ProjectInfo[]
  collapsed: boolean
  onToggle: () => void
  onNavigate: (project: ProjectInfo) => void
  onStop: (project: ProjectInfo) => Promise<void>
  onRestart: (project: ProjectInfo) => Promise<void>
  onError: (message: string) => void
}) {
  const contentId = `sidebar-project-group-${projects[0]?.id ?? 'empty'}`
  return (
    <>
      <button
        type="button"
        onClick={onToggle}
        aria-expanded={!collapsed}
        aria-controls={contentId}
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 5,
          width: '100%',
          padding: '4px 16px',
          background: 'transparent',
          border: 'none',
          cursor: 'pointer',
          textAlign: 'left',
        }}
      >
        <span
          style={{
            fontSize: 8,
            color: 'var(--color-muted-foreground)',
            transform: collapsed ? 'rotate(-90deg)' : 'rotate(0deg)',
          }}
        >
          ▼
        </span>
        <span
          style={{
            flex: 1,
            fontSize: 10,
            fontWeight: 700,
            color: 'var(--color-muted-foreground)',
            letterSpacing: '0.05em',
          }}
        >
          {category}
        </span>
        <span style={{ fontSize: 9, color: 'var(--color-muted-foreground)', opacity: 0.6 }}>
          {projects.length}
        </span>
      </button>
      {!collapsed && (
        <div id={contentId}>
          {projects.map(project => (
            <SidebarProject
              key={project.id}
              project={project}
              onNavigate={() => onNavigate(project)}
              onStop={() => onStop(project)}
              onRestart={() => onRestart(project)}
              onError={onError}
            />
          ))}
        </div>
      )}
    </>
  )
}

function SidebarProject({
  project,
  onNavigate,
  onStop,
  onRestart,
  onError,
}: {
  project: ProjectInfo
  onNavigate: () => void
  onStop: () => Promise<void>
  onRestart: () => Promise<void>
  onError: (message: string) => void
}) {
  const [hovered, setHovered] = useState(false)
  const [busy, setBusy] = useState<'stop' | 'restart' | null>(null)

  async function run(action: 'stop' | 'restart') {
    setBusy(action)
    try {
      if (action === 'stop') await onStop()
      else await onRestart()
    } catch (actionError) {
      onError(actionError instanceof Error ? actionError.message : '项目操作失败')
    } finally {
      setBusy(null)
    }
  }

  return (
    <div
      style={{ position: 'relative', display: 'flex', alignItems: 'center' }}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
    >
      <button
        onClick={onNavigate}
        title={`${project.display_name} — ${projectStatusLabel(project.status)}`}
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 7,
          flex: 1,
          minWidth: 0,
          padding: '5px 16px 5px 24px',
          background: hovered ? 'var(--color-accent)' : 'transparent',
          border: 'none',
          cursor: 'pointer',
          color: 'var(--color-foreground)',
          fontSize: 12,
          textAlign: 'left',
        }}
      >
        <span style={{ color: projectStatusColor(project.status), fontSize: 9, flexShrink: 0 }}>
          ●
        </span>
        <span
          style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', flex: 1 }}
        >
          {project.display_name}
        </span>
        {project.process_count > 1 && (
          <span style={{ fontSize: 9, color: 'var(--color-muted-foreground)' }}>
            {project.process_count}
          </span>
        )}
      </button>
      {project.kind !== 'desktop' && (
        <div
          style={{ display: 'flex', gap: 1, paddingRight: 6, background: 'var(--color-accent)' }}
        >
          <SidebarActionBtn
            icon={Square}
            title={busy === 'stop' ? '停止中…' : '停止项目'}
            onClick={() => void run('stop')}
            color="#f87171"
            disabled={busy !== null}
          />
          <SidebarActionBtn
            icon={RotateCcw}
            title={busy === 'restart' ? '重启中…' : '重启项目'}
            onClick={() => void run('restart')}
            color="#4ade80"
            disabled={busy !== null}
          />
        </div>
      )}
    </div>
  )
}

function SidebarActionBtn({
  icon: Icon,
  title,
  onClick,
  color,
  disabled,
}: {
  icon: LucideIcon
  title: string
  onClick: () => void
  color?: string
  disabled?: boolean
}) {
  return (
    <button
      title={title}
      disabled={disabled}
      onClick={event => {
        event.stopPropagation()
        onClick()
      }}
      style={{
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        width: 20,
        height: 20,
        padding: 0,
        border: 'none',
        borderRadius: 3,
        background: 'transparent',
        color: color ?? 'var(--color-muted-foreground)',
        cursor: disabled ? 'wait' : 'pointer',
        opacity: disabled ? 0.45 : 1,
      }}
    >
      <Icon size={11} />
    </button>
  )
}
