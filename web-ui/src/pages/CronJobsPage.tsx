// @group BusinessLogic : Cron Jobs list view — namespace-grouped table, mirrors ProcessesPage style

import { useEffect, useRef, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { Play, Square, ScrollText, Pencil, Trash2, Bell } from 'lucide-react'
import { api } from '@/lib/api'
import { useDialog } from '@/hooks/useDialog'
import { Dialog } from '@/components/Dialog'
import { ProcessNotifModal, NsNotifModal } from '@/components/NotifModal'
import { formatNextRun, formatBytes, formatCpu, processStatusLabel, statusColor } from '@/lib/utils'
import { filterCronJobsByNamespace } from '@/lib/cronJobs'
import type { AppSettings } from '@/lib/settings'
import type { CronRun, ProcessInfo } from '@/types'

interface Props {
  processes: ProcessInfo[]
  reload: () => void
  settings: AppSettings
  namespaceFilter?: string | null
}

// @group Utilities > CronDescription : Human-readable schedule description
function describeSchedule(expr: string): string {
  const parts = expr.trim().split(/\s+/)
  if (parts.length !== 5) return expr
  const [min, hour, dom, month, dow] = parts
  if (expr === '* * * * *') return '每分钟'
  if (min.startsWith('*/') && hour === '*' && dom === '*' && month === '*' && dow === '*')
    return `每 ${min.slice(2)} 分钟`
  if (min === '0' && hour === '*' && dom === '*' && month === '*' && dow === '*') return '每小时'
  if (min !== '*' && hour !== '*' && dom === '*' && month === '*' && dow === '*')
    return `每天 ${hour.padStart(2, '0')}:${min.padStart(2, '0')}`
  if (min !== '*' && hour !== '*' && dom === '*' && month === '*' && dow !== '*') {
    const days = ['周日', '周一', '周二', '周三', '周四', '周五', '周六']
    return `${days[parseInt(dow)] ?? dow} ${hour.padStart(2, '0')}:${min.padStart(2, '0')}`
  }
  if (min !== '*' && hour !== '*' && dom !== '*' && month === '*' && dow === '*')
    return `每月 ${dom} 日`
  return expr
}

// @group Utilities > LastRun : Format last cron run entry
function LastRunCell({ history, now }: { history: CronRun[]; now: number }) {
  if (history.length === 0) return <span style={{ color: 'var(--color-muted-foreground)' }}>—</span>
  const last = history[history.length - 1]
  const d = new Date(last.run_at)
  const diffSecs = Math.floor((now - d.getTime()) / 1000)
  let ago: string
  if (diffSecs < 60) ago = `${diffSecs}秒前`
  else if (diffSecs < 3600) ago = `${Math.floor(diffSecs / 60)}分钟前`
  else if (diffSecs < 86400) ago = `${Math.floor(diffSecs / 3600)}小时前`
  else ago = d.toLocaleDateString('zh-CN', { month: 'short', day: 'numeric' })

  const code = last.exit_code
  const pillColor =
    code === null ? '#888' : code === 0 ? 'var(--color-status-running)' : 'var(--color-destructive)'
  const pillBg =
    code === null
      ? 'rgba(128,128,128,0.15)'
      : code === 0
        ? 'rgba(34,197,94,0.15)'
        : 'rgba(239,68,68,0.15)'

  return (
    <span style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
      <span style={{ color: 'var(--color-muted-foreground)' }}>{ago}</span>
      <span
        style={{
          display: 'inline-block',
          padding: '1px 6px',
          borderRadius: 4,
          fontSize: 10,
          fontWeight: 700,
          color: pillColor,
          background: pillBg,
        }}
      >
        {code === null ? '?' : `退出码 ${code}`}
      </span>
      {last.duration_secs > 0 && (
        <span style={{ fontSize: 11, color: 'var(--color-muted-foreground)' }}>
          {last.duration_secs} 秒
        </span>
      )}
    </span>
  )
}

export default function CronJobsPage({ processes, reload, settings, namespaceFilter }: Props) {
  const navigate = useNavigate()
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set())
  const [notifProcess, setNotifProcess] = useState<ProcessInfo | null>(null)
  const [notifNs, setNotifNs] = useState<string | null>(null)
  const { dialogState, confirm, danger, handleConfirm, handleCancel } = useDialog()
  const [now, setNow] = useState(() => Date.now())
  const [actionError, setActionError] = useState<string | null>(null)

  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 30_000)
    return () => window.clearInterval(timer)
  }, [])

  const cronJobs = filterCronJobsByNamespace(processes, namespaceFilter)

  // @group BusinessLogic > Grouping : Group cron jobs by namespace, default first
  const groups = new Map<string, ProcessInfo[]>()
  for (const p of cronJobs) {
    const ns = p.namespace || 'default'
    if (!groups.has(ns)) groups.set(ns, [])
    groups.get(ns)!.push(p)
  }
  const sortedNs = [...groups.keys()].sort((a, b) =>
    a === 'default' ? -1 : b === 'default' ? 1 : a.localeCompare(b)
  )

  function toggleNs(ns: string) {
    setCollapsed(prev => {
      const next = new Set(prev)
      if (next.has(ns)) next.delete(ns)
      else next.add(ns)
      return next
    })
  }

  if (cronJobs.length === 0) {
    return (
      <div style={{ padding: 40, textAlign: 'center' }}>
        <div style={{ fontSize: 32, marginBottom: 12 }}>⏱</div>
        <div style={{ fontWeight: 600, fontSize: 16, marginBottom: 8 }}>暂无定时任务</div>
        <div style={{ color: 'var(--color-muted-foreground)', fontSize: 13, marginBottom: 20 }}>
          按时间计划自动运行脚本。
        </div>
        <button
          onClick={() => navigate('/cron-jobs/new')}
          style={{
            padding: '8px 20px',
            fontSize: 13,
            fontWeight: 600,
            background: 'var(--color-primary)',
            border: 'none',
            borderRadius: 5,
            cursor: 'pointer',
            color: '#fff',
          }}
        >
          ⏱ 创建定时任务
        </button>
      </div>
    )
  }

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%' }}>
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
      {actionError && (
        <p style={{ margin: '6px 20px', color: 'var(--color-status-crashed)', fontSize: 12 }}>
          {actionError}
        </p>
      )}

      {notifProcess && (
        <ProcessNotifModal process={notifProcess} onClose={() => setNotifProcess(null)} />
      )}
      {notifNs && <NsNotifModal ns={notifNs} onClose={() => setNotifNs(null)} />}

      {/* Header */}
      <div
        style={{
          padding: '16px 20px 10px',
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          borderBottom: '1px solid var(--color-border)',
          flexShrink: 0,
        }}
      >
        <h2 style={{ fontSize: 16, fontWeight: 600 }}>
          定时任务{' '}
          <span style={{ fontWeight: 400, color: 'var(--color-muted-foreground)', fontSize: 13 }}>
            ({cronJobs.length})
          </span>
        </h2>
        <div style={{ display: 'flex', gap: 8 }}>
          <button onClick={reload} style={smallBtnStyle}>
            ↻ 刷新
          </button>
          <button
            onClick={() => navigate('/cron-jobs/new')}
            style={{
              ...smallBtnStyle,
              background: 'var(--color-primary)',
              color: '#fff',
              border: 'none',
              fontWeight: 600,
            }}
          >
            ⏱ 新建定时任务
          </button>
        </div>
      </div>

      {/* Table */}
      <div style={{ flex: 1, overflow: 'auto' }}>
        <table style={{ width: '100%', borderCollapse: 'collapse', fontSize: 13 }}>
          <thead>
            <tr
              style={{
                background: 'var(--color-card)',
                borderBottom: '1px solid var(--color-border)',
              }}
            >
              {['名称', '计划', '状态', '下次运行', '上次运行', 'CPU', '内存', '操作'].map(h => (
                <Th key={h}>{h}</Th>
              ))}
            </tr>
          </thead>
          <tbody>
            {sortedNs.map(ns => {
              const jobs = groups.get(ns)!
              const isCollapsed = collapsed.has(ns)
              return [
                // Namespace header row
                <tr
                  key={`ns-${ns}`}
                  onClick={() => toggleNs(ns)}
                  style={{
                    background: 'var(--color-muted)',
                    cursor: 'pointer',
                    userSelect: 'none',
                  }}
                >
                  <td colSpan={8} style={{ padding: '6px 12px' }}>
                    <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                      <span style={{ fontSize: 10, color: 'var(--color-muted-foreground)' }}>
                        {isCollapsed ? '▶' : '▼'}
                      </span>
                      <span style={{ fontWeight: 600, fontSize: 12 }}>{ns}</span>
                      <span style={{ fontSize: 11, color: 'var(--color-muted-foreground)' }}>
                        {jobs.length} 个任务
                      </span>
                      <span
                        onClick={e => e.stopPropagation()}
                        style={{
                          marginLeft: 'auto',
                          display: 'flex',
                          gap: 5,
                          alignItems: 'center',
                        }}
                      >
                        <button
                          onClick={() => setNotifNs(ns)}
                          title="命名空间通知"
                          style={{
                            padding: '2px 5px',
                            background: 'transparent',
                            border: '1px solid var(--color-border)',
                            borderRadius: 4,
                            cursor: 'pointer',
                            display: 'flex',
                            alignItems: 'center',
                          }}
                        >
                          <Bell size={11} style={{ color: '#a78bfa' }} />
                        </button>
                      </span>
                    </div>
                  </td>
                </tr>,
                // Job rows
                ...(!isCollapsed
                  ? jobs.map(p => (
                      <CronJobRow
                        key={p.id}
                        p={p}
                        now={now}
                        reload={reload}
                        confirmDelete={settings.confirmBeforeDelete}
                        onConfirm={confirm}
                        onDanger={danger}
                        onOpenDetail={() => navigate(`/processes/${p.id}`)}
                        onEdit={() => navigate(`/edit/${p.id}`)}
                        onOpenNotif={() => setNotifProcess(p)}
                        onError={setActionError}
                      />
                    ))
                  : []),
              ]
            })}
          </tbody>
        </table>
      </div>
    </div>
  )
}

// @group BusinessLogic > CronJobRow : Single cron job table row
function CronJobRow({
  p,
  now,
  reload,
  confirmDelete,
  onConfirm,
  onDanger,
  onOpenDetail,
  onEdit,
  onOpenNotif,
  onError,
}: {
  p: ProcessInfo
  now: number
  reload: () => void
  confirmDelete: boolean
  onConfirm: (title: string, message?: string) => Promise<boolean>
  onDanger: (title: string, message?: string, confirmLabel?: string) => Promise<boolean>
  onOpenDetail: () => void
  onEdit: () => void
  onOpenNotif: () => void
  onError: (message: string | null) => void
}) {
  const isActive = p.status === 'running' || p.status === 'sleeping'
  const hasNotify =
    !!p.notify?.webhook?.enabled || !!p.notify?.slack?.enabled || !!p.notify?.teams?.enabled
  const actionPending = useRef(false)

  async function runAction(action: () => Promise<unknown>, delayMs = 0) {
    if (actionPending.current) return
    actionPending.current = true
    try {
      await action()
      onError(null)
      if (delayMs > 0) setTimeout(reload, delayMs)
      else reload()
    } catch (error) {
      onError(error instanceof Error ? error.message : '定时任务操作失败')
    } finally {
      actionPending.current = false
    }
  }

  async function doRunNow() {
    await runAction(() => api.startStopped(p.id), 400)
  }
  async function doStop() {
    const ok = await onConfirm(`停止“${p.name}”？`, '定时任务将停止，重新启动后计划才会恢复。')
    if (!ok) return
    await runAction(() => api.stopProcess(p.id))
  }
  async function doDelete() {
    if (confirmDelete) {
      const ok = await onDanger(`删除“${p.name}”？`, '这将永久删除该定时任务及其配置。', '删除')
      if (!ok) return
    }
    await runAction(() => api.deleteProcess(p.id), 300)
  }

  return (
    <tr
      style={{ borderBottom: '1px solid var(--color-border)' }}
      onMouseEnter={e => (e.currentTarget.style.background = 'var(--color-accent)')}
      onMouseLeave={e => (e.currentTarget.style.background = 'transparent')}
    >
      {/* Name + command */}
      <Td>
        <strong style={{ cursor: 'pointer' }} onClick={onOpenDetail}>
          {p.name}
        </strong>
        <div
          style={{
            fontSize: 11,
            color: 'var(--color-muted-foreground)',
            marginTop: 2,
            fontFamily: 'monospace',
          }}
        >
          {p.script} {p.args.join(' ')}
        </div>
      </Td>

      {/* Schedule */}
      <Td>
        <div style={{ fontFamily: 'monospace', fontSize: 12 }}>{p.cron}</div>
        <div style={{ fontSize: 11, color: 'var(--color-muted-foreground)', marginTop: 2 }}>
          {describeSchedule(p.cron!)}
        </div>
      </Td>

      {/* Status */}
      <Td>
        <span
          style={{
            display: 'inline-flex',
            alignItems: 'center',
            gap: 4,
            fontSize: 12,
            color: statusColor(p.status),
          }}
        >
          ● {processStatusLabel(p.status)}
        </span>
      </Td>

      {/* Next Run */}
      <Td style={{ color: 'var(--color-status-sleeping)' }}>
        {p.status === 'stopped' || p.status === 'errored' ? (
          <span style={{ color: 'var(--color-muted-foreground)' }}>已暂停</span>
        ) : (
          formatNextRun(p.cron_next_run)
        )}
      </Td>

      {/* Last Run */}
      <Td>
        <LastRunCell history={p.cron_run_history} now={now} />
      </Td>

      {/* CPU */}
      <Td style={{ color: 'var(--color-muted-foreground)' }}>
        {p.cpu_percent != null ? formatCpu(p.cpu_percent) : '-'}
      </Td>

      {/* Mem */}
      <Td style={{ color: 'var(--color-muted-foreground)' }}>
        {p.memory_bytes != null ? formatBytes(p.memory_bytes) : '-'}
      </Td>

      {/* Actions */}
      <Td>
        <div style={{ display: 'flex', gap: 3, flexWrap: 'nowrap' }}>
          {isActive ? (
            <ActionBtn label="停止" icon={Square} onClick={doStop} color="#f87171" />
          ) : (
            <ActionBtn label="立即运行" icon={Play} onClick={doRunNow} color="#4ade80" />
          )}
          <ActionBtn label="日志" icon={ScrollText} onClick={onOpenDetail} color="#60a5fa" />
          <ActionBtn label="编辑" icon={Pencil} onClick={onEdit} color="#34d399" />
          <ActionBtn
            label="通知"
            icon={Bell}
            onClick={onOpenNotif}
            color="#a78bfa"
            badge={hasNotify ? '●' : undefined}
          />
          <ActionBtn label="删除" icon={Trash2} onClick={doDelete} danger />
        </div>
      </Td>
    </tr>
  )
}

// @group Utilities > UI helpers
function Th({ children }: { children: React.ReactNode }) {
  return (
    <th
      style={{
        padding: '8px 12px',
        textAlign: 'left',
        fontWeight: 600,
        fontSize: 11,
        color: 'var(--color-muted-foreground)',
        letterSpacing: '0.04em',
        whiteSpace: 'nowrap',
      }}
    >
      {children}
    </th>
  )
}

function Td({
  children,
  style,
  title,
}: {
  children?: React.ReactNode
  style?: React.CSSProperties
  title?: string
}) {
  return (
    <td title={title} style={{ padding: '7px 12px', whiteSpace: 'nowrap', ...style }}>
      {children}
    </td>
  )
}

function ActionBtn({
  label,
  icon: Icon,
  onClick,
  danger,
  color,
  badge,
}: {
  label: string
  icon: React.ElementType
  onClick: () => void
  danger?: boolean
  color?: string
  badge?: string
}) {
  const iconColor = danger ? 'var(--color-destructive)' : (color ?? 'var(--color-muted-foreground)')
  return (
    <button
      title={label}
      onClick={onClick}
      style={{
        position: 'relative',
        padding: 0,
        width: 26,
        height: 26,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        background: 'var(--color-secondary)',
        border: '1px solid var(--color-border)',
        borderRadius: 4,
        cursor: 'pointer',
        flexShrink: 0,
        color: iconColor,
      }}
    >
      <Icon size={13} strokeWidth={1.75} />
      {badge && (
        <span
          style={{
            position: 'absolute',
            top: -3,
            right: -3,
            fontSize: 8,
            color: iconColor,
            lineHeight: 1,
          }}
        >
          {badge}
        </span>
      )}
    </button>
  )
}

const smallBtnStyle: React.CSSProperties = {
  padding: '4px 10px',
  fontSize: 12,
  background: 'var(--color-secondary)',
  border: '1px solid var(--color-border)',
  borderRadius: 5,
  cursor: 'pointer',
  color: 'var(--color-foreground)',
}
