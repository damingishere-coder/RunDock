// @group BusinessLogic : Log Library — browse log history for every process

import { useEffect, useRef, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { ScrollText, Search, CalendarDays, Trash2, RefreshCw } from 'lucide-react'
import { api } from '@/lib/api'
import { mapSettledWithConcurrency } from '@/lib/async'
import { statusColor } from '@/lib/utils'
import type { ProcessInfo } from '@/types'

interface Props {
  processes: ProcessInfo[]
  reload: () => void
}

// @group Types : Per-process log metadata loaded async
type LogMeta = {
  dates: string[]
  hasCurrent: boolean
  loading: boolean
  error?: string
}

export default function LogLibraryPage({ processes, reload }: Props) {
  const navigate = useNavigate()
  const [filter, setFilter] = useState('')
  const [logMeta, setLogMeta] = useState<Record<string, LogMeta>>({})
  const [flushing, setFlushing] = useState<string | null>(null)
  const [logRefreshVersion, setLogRefreshVersion] = useState(0)
  const loadGenerationRef = useRef(0)
  const loadAbortRef = useRef<AbortController | null>(null)
  const flushInFlightRef = useRef(false)
  const processIdsKey = JSON.stringify(
    processes.map(process => process.id).sort((left, right) => left.localeCompare(right))
  )

  // @group BusinessLogic > DataFetch : Load log dates for all processes in parallel
  useEffect(() => {
    const processIds = JSON.parse(processIdsKey) as string[]
    const generation = ++loadGenerationRef.current
    loadAbortRef.current?.abort()
    if (!processIds.length) {
      setLogMeta({})
      return
    }
    const controller = new AbortController()
    loadAbortRef.current = controller

    // Initialise all as loading
    const initial: Record<string, LogMeta> = {}
    for (const id of processIds) initial[id] = { dates: [], hasCurrent: false, loading: true }
    setLogMeta(initial)

    void mapSettledWithConcurrency(processIds, 6, async id => {
      const data = await api.getLogDates(id, { signal: controller.signal })
      return { id, dates: data.dates, hasCurrent: data.has_current, loading: false }
    }).then(results => {
      if (controller.signal.aborted || generation !== loadGenerationRef.current) return
      const map: Record<string, LogMeta> = {}
      results.forEach((result, index) => {
        const id = processIds[index]
        if (result.status === 'fulfilled') {
          map[result.value.id] = result.value
        } else {
          map[id] = {
            dates: [],
            hasCurrent: false,
            loading: false,
            error: result.reason instanceof Error ? result.reason.message : '日志信息加载失败',
          }
        }
      })
      setLogMeta(map)
    })
    return () => controller.abort()
  }, [processIdsKey, logRefreshVersion])

  // @group BusinessLogic > FlushLogs : Delete all log files for a process
  async function handleFlush(p: ProcessInfo) {
    if (flushInFlightRef.current) return
    if (!confirm(`删除“${p.name}”的全部日志文件？`)) return
    flushInFlightRef.current = true
    const generation = ++loadGenerationRef.current
    loadAbortRef.current?.abort()
    setFlushing(p.id)
    try {
      await api.deleteLogs(p.id)
      setLogRefreshVersion(version => version + 1)
    } catch (flushError: unknown) {
      if (generation !== loadGenerationRef.current) return
      setLogMeta(prev => ({
        ...prev,
        [p.id]: {
          dates: prev[p.id]?.dates ?? [],
          hasCurrent: prev[p.id]?.hasCurrent ?? false,
          loading: false,
          error:
            flushError instanceof Error
              ? `日志删除或刷新失败：${flushError.message}`
              : '日志删除或刷新失败',
        },
      }))
    } finally {
      flushInFlightRef.current = false
      setFlushing(null)
    }
  }

  // @group BusinessLogic > Filter : Namespace-grouped filtered process list
  const visible = processes.filter(
    p =>
      filter === '' ||
      p.name.toLowerCase().includes(filter.toLowerCase()) ||
      p.namespace.toLowerCase().includes(filter.toLowerCase())
  )

  const groups = new Map<string, ProcessInfo[]>()
  for (const p of visible) {
    const ns = p.namespace || 'default'
    if (!groups.has(ns)) groups.set(ns, [])
    groups.get(ns)!.push(p)
  }
  const sortedNs = [...groups.keys()].sort((a, b) =>
    a === 'default' ? -1 : b === 'default' ? 1 : a.localeCompare(b)
  )

  const totalDates = Object.values(logMeta).reduce(
    (sum, m) => sum + m.dates.length + (m.hasCurrent ? 1 : 0),
    0
  )
  const stillLoading = Object.values(logMeta).some(m => m.loading)

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%' }}>
      {/* ── Header ── */}
      <div
        style={{
          padding: '16px 20px 12px',
          borderBottom: '1px solid var(--color-border)',
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          gap: 12,
        }}
      >
        <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
          <ScrollText size={17} style={{ color: 'var(--color-primary)' }} />
          <h2 style={{ fontSize: 16, fontWeight: 600, margin: 0 }}>日志库</h2>
          <span style={{ fontSize: 11, color: 'var(--color-muted-foreground)' }}>
            {processes.length} 个进程
            {' · '}
            {stillLoading ? '统计中…' : `${totalDates} 个日志日期`}
          </span>
        </div>
        <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
          {/* Search */}
          <div style={{ position: 'relative', display: 'flex', alignItems: 'center' }}>
            <Search
              size={12}
              style={{
                position: 'absolute',
                left: 8,
                color: 'var(--color-muted-foreground)',
                pointerEvents: 'none',
              }}
            />
            <input
              value={filter}
              onChange={e => setFilter(e.target.value)}
              placeholder="按名称或命名空间筛选…"
              style={{
                paddingLeft: 26,
                paddingRight: 10,
                paddingTop: 5,
                paddingBottom: 5,
                fontSize: 12,
                width: 220,
                background: 'var(--color-secondary)',
                border: '1px solid var(--color-border)',
                borderRadius: 5,
                color: 'var(--color-foreground)',
                outline: 'none',
              }}
            />
          </div>
          <button
            type="button"
            onClick={reload}
            title="刷新进程列表"
            aria-label="刷新进程列表"
            style={smallBtn}
          >
            <RefreshCw size={13} />
          </button>
        </div>
      </div>

      {/* ── Body ── */}
      <div style={{ flex: 1, overflow: 'auto', padding: '12px 20px' }}>
        {processes.length === 0 ? (
          <div style={{ padding: 32, color: 'var(--color-muted-foreground)', textAlign: 'center' }}>
            尚未注册进程。
          </div>
        ) : visible.length === 0 ? (
          <div style={{ padding: 32, color: 'var(--color-muted-foreground)', textAlign: 'center' }}>
            没有匹配“{filter}”的进程。
          </div>
        ) : (
          sortedNs.map(ns => {
            const procs = groups.get(ns)!
            return (
              <div key={ns} style={{ marginBottom: 20 }}>
                {/* Namespace label */}
                <div
                  style={{
                    fontSize: 10,
                    fontWeight: 700,
                    letterSpacing: '0.08em',
                    color: 'var(--color-muted-foreground)',
                    textTransform: 'uppercase',
                    marginBottom: 6,
                    paddingLeft: 2,
                  }}
                >
                  {ns} · {procs.length} 个进程
                </div>

                {/* Process cards */}
                <div
                  style={{
                    background: 'var(--color-card)',
                    border: '1px solid var(--color-border)',
                    borderRadius: 8,
                    overflow: 'hidden',
                  }}
                >
                  {procs.map((p, i) => {
                    const meta = logMeta[p.id]
                    const isLast = i === procs.length - 1
                    return (
                      <LogRow
                        key={p.id}
                        p={p}
                        meta={meta}
                        isLast={isLast}
                        isFlushing={flushing === p.id}
                        onView={() => navigate(`/processes/${p.id}`)}
                        onViewToday={() => navigate(`/processes/${p.id}`)}
                        onFlush={() => handleFlush(p)}
                      />
                    )
                  })}
                </div>
              </div>
            )
          })
        )}
      </div>
    </div>
  )
}

// @group BusinessLogic > LogRow : Single process row in the log library
function LogRow({
  p,
  meta,
  isLast,
  isFlushing,
  onView,
  onViewToday,
  onFlush,
}: {
  p: ProcessInfo
  meta: LogMeta | undefined
  isLast: boolean
  isFlushing: boolean
  onView: () => void
  onViewToday: () => void
  onFlush: () => void
}) {
  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: 12,
        padding: '10px 14px',
        borderBottom: isLast ? 'none' : '1px solid var(--color-border)',
        cursor: 'default',
      }}
      onMouseEnter={e => (e.currentTarget.style.background = 'var(--color-accent)')}
      onMouseLeave={e => (e.currentTarget.style.background = 'transparent')}
    >
      {/* Status dot + name */}
      <div style={{ display: 'flex', alignItems: 'center', gap: 7, minWidth: 160 }}>
        <span style={{ color: statusColor(p.status), fontSize: 9, flexShrink: 0 }}>●</span>
        <button
          type="button"
          style={{
            fontWeight: 600,
            fontSize: 13,
            cursor: 'pointer',
            border: 'none',
            background: 'transparent',
            color: 'inherit',
            padding: 0,
          }}
          onClick={onView}
          title={`查看 ${p.name} 的日志`}
        >
          {p.name}
        </button>
      </div>

      {/* Script (truncated) */}
      <span
        style={{
          flex: 1,
          fontSize: 11,
          color: 'var(--color-muted-foreground)',
          overflow: 'hidden',
          textOverflow: 'ellipsis',
          whiteSpace: 'nowrap',
          fontFamily: 'monospace',
        }}
        title={p.script}
      >
        {p.script}
      </span>

      {/* Log dates badges */}
      <div style={{ display: 'flex', alignItems: 'center', gap: 6, flexShrink: 0 }}>
        {meta?.loading ? (
          <span style={{ fontSize: 11, color: 'var(--color-muted-foreground)' }}>…</span>
        ) : meta?.error ? (
          <span title={meta.error} style={{ fontSize: 11, color: 'var(--color-status-crashed)' }}>
            加载失败
          </span>
        ) : meta && (meta.hasCurrent || meta.dates.length > 0) ? (
          <>
            <CalendarDays size={11} style={{ color: 'var(--color-muted-foreground)' }} />
            {meta.hasCurrent && <TodayBadge onClick={onViewToday} />}
            {meta.dates.length > 0 && <DateBadges dates={meta.dates} onView={onView} />}
          </>
        ) : (
          <span style={{ fontSize: 11, color: 'var(--color-muted-foreground)' }}>无日志</span>
        )}
      </div>

      {/* Actions */}
      <div style={{ display: 'flex', gap: 6, flexShrink: 0 }}>
        <button onClick={onView} style={viewBtn}>
          查看日志
        </button>
        <button
          type="button"
          onClick={onFlush}
          disabled={isFlushing}
          title="删除此进程的全部日志文件"
          aria-label="删除此进程的全部日志文件"
          style={{
            ...iconBtnBase,
            color: 'var(--color-destructive)',
            opacity: isFlushing ? 0.5 : 1,
          }}
        >
          <Trash2 size={13} />
        </button>
      </div>
    </div>
  )
}

// @group BusinessLogic > TodayBadge : Green "Today" chip shown when current out.log/err.log exists
function TodayBadge({ onClick }: { onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={onClick}
      title="查看今天的实时日志"
      style={{
        fontSize: 10,
        fontWeight: 600,
        padding: '1px 7px',
        borderRadius: 10,
        background: 'rgba(34,197,94,0.12)',
        color: 'var(--color-status-running)',
        border: '1px solid rgba(34,197,94,0.3)',
        cursor: 'pointer',
        whiteSpace: 'nowrap',
      }}
    >
      今天
    </button>
  )
}

// @group BusinessLogic > DateBadges : Show up to 3 date chips, then "+N more"
function DateBadges({ dates, onView }: { dates: string[]; onView: () => void }) {
  const SHOW = 3
  const visible = dates.slice(-SHOW).reverse() // most recent first
  const extra = dates.length - SHOW

  return (
    <div style={{ display: 'flex', gap: 4, alignItems: 'center', flexWrap: 'nowrap' }}>
      {visible.map(d => (
        <button
          type="button"
          key={d}
          onClick={onView}
          title={`查看 ${d} 的日志`}
          style={{
            fontSize: 10,
            fontWeight: 500,
            padding: '1px 6px',
            borderRadius: 10,
            background: 'rgba(79,156,249,0.12)',
            color: 'var(--color-status-sleeping)',
            border: '1px solid rgba(79,156,249,0.25)',
            cursor: 'pointer',
            whiteSpace: 'nowrap',
          }}
        >
          {d}
        </button>
      ))}
      {extra > 0 && (
        <span
          style={{
            fontSize: 10,
            color: 'var(--color-muted-foreground)',
            padding: '1px 4px',
          }}
        >
          还有 {extra} 个
        </span>
      )}
    </div>
  )
}

// @group Utilities > Styles : Shared button styles
const smallBtn: React.CSSProperties = {
  padding: '5px 7px',
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'center',
  background: 'var(--color-secondary)',
  border: '1px solid var(--color-border)',
  borderRadius: 5,
  cursor: 'pointer',
  color: 'var(--color-foreground)',
}

const viewBtn: React.CSSProperties = {
  padding: '3px 10px',
  fontSize: 11,
  fontWeight: 500,
  background: 'var(--color-secondary)',
  border: '1px solid var(--color-border)',
  borderRadius: 4,
  cursor: 'pointer',
  color: 'var(--color-foreground)',
  whiteSpace: 'nowrap',
}

const iconBtnBase: React.CSSProperties = {
  padding: '4px',
  width: 26,
  height: 26,
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'center',
  background: 'transparent',
  border: '1px solid var(--color-border)',
  borderRadius: 4,
  cursor: 'pointer',
}
