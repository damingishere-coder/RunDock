// @group BusinessLogic : Processes list view — stats strip, namespace pills, table + card grid

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import {
  Play, Square, RotateCcw, ScrollText, Pencil, Trash2, FileKey, Bell, Copy, SquareTerminal,
  ChevronUp, ChevronDown, ChevronsUpDown, LayoutGrid, List, GitBranch, Power, PowerOff,
} from 'lucide-react'
import { api } from '@/lib/api'
import { useDialog } from '@/hooks/useDialog'
import { Dialog } from '@/components/Dialog'
import { EnvFilePanel } from '@/components/EnvFilePanel'
import { ProcessNotifModal, NsNotifModal } from '@/components/NotifModal'
import { WebPortButton } from '@/components/WebPortButton'
import { formatLastRun, formatNextRun, formatUptime, formatBytes, formatCpu, processStatusLabel, statusColor } from '@/lib/utils'
import { getActiveServer, type RemoteServer } from '@/lib/servers'
import { listeningPortsByManagedPid, type PortScanEntry } from '@/lib/processWeb'
import type { AppSettings } from '@/lib/settings'
import type { ProcessInfo } from '@/types'

interface Props {
  processes: ProcessInfo[]
  reload: () => void
  settings: AppSettings
  namespaceFilter?: string
  onOpenTerminal?: (cwd: string, name?: string) => void
}

type SortCol = 'name' | 'status' | 'cpu' | 'memory' | 'restarts' | 'uptime' | null
type SortDir = 'asc' | 'desc'
type ViewMode = 'table' | 'cards'

export default function ProcessesPage({ processes, reload, settings, namespaceFilter, onOpenTerminal }: Props) {
  const [search, setSearch]           = useState('')
  const [statusFilter, setStatusFilter] = useState<string>('all')
  const [nsFilter, setNsFilter]       = useState<string>('all')
  const [sortCol, setSortCol]         = useState<SortCol>(null)
  const [sortDir, setSortDir]         = useState<SortDir>('asc')
  const [viewMode, setViewMode]       = useState<ViewMode>('table')

  // @group BusinessLogic > ViewMode : Load persisted view-mode from daemon on mount
  useEffect(() => {
    api.getUiSettings().then(s => {
      if (s.viewMode === 'card' || s.viewMode === 'table') setViewMode(s.viewMode as ViewMode)
    }).catch(() => {})
  }, [])

  function handleSort(col: SortCol) {
    if (sortCol === col) setSortDir(d => d === 'asc' ? 'desc' : 'asc')
    else { setSortCol(col); setSortDir('asc') }
  }
  function setView(mode: ViewMode) {
    setViewMode(mode)
    api.saveUiSettings({ viewMode: mode }).catch(() => {})
  }


  // @group BusinessLogic > Filtering : Base → ns → search → status → sort
  const baseProcesses = namespaceFilter
    ? processes.filter(p => (p.namespace || 'default') === namespaceFilter)
    : processes

  const displayedProcesses = useMemo(() => {
    let list = baseProcesses
    if (nsFilter !== 'all') list = list.filter(p => (p.namespace || 'default') === nsFilter)
    if (search.trim()) {
      const q = search.toLowerCase()
      list = list.filter(p => p.name.toLowerCase().includes(q) || p.script.toLowerCase().includes(q))
    }
    if (statusFilter !== 'all') list = list.filter(p => p.status === statusFilter)
    if (sortCol) {
      list = [...list].sort((a, b) => {
        let av: number | string = 0, bv: number | string = 0
        if (sortCol === 'name')     { av = a.name; bv = b.name }
        if (sortCol === 'status')   { av = a.status; bv = b.status }
        if (sortCol === 'cpu')      { av = a.cpu_percent ?? -1; bv = b.cpu_percent ?? -1 }
        if (sortCol === 'memory')   { av = a.memory_bytes ?? -1; bv = b.memory_bytes ?? -1 }
        if (sortCol === 'restarts') { av = a.restart_count; bv = b.restart_count }
        if (sortCol === 'uptime')   { av = a.uptime_secs ?? -1; bv = b.uptime_secs ?? -1 }
        const cmp = typeof av === 'string' ? av.localeCompare(bv as string) : (av as number) - (bv as number)
        return sortDir === 'asc' ? cmp : -cmp
      })
    }
    return list
  }, [baseProcesses, nsFilter, search, statusFilter, sortCol, sortDir])

  // @group BusinessLogic > Stats : Aggregate metrics for the stats strip
  const stats = useMemo(() => ({
    total:    baseProcesses.length,
    running:  baseProcesses.filter(p => p.status === 'running' || p.status === 'watching').length,
    crashed:  baseProcesses.filter(p => p.status === 'crashed' || p.status === 'errored').length,
    stopped:  baseProcesses.filter(p => p.status === 'stopped').length,
    sleeping: baseProcesses.filter(p => p.status === 'sleeping').length,
    totalCpu: baseProcesses.reduce((s, p) => s + (p.cpu_percent ?? 0), 0),
    totalMem: baseProcesses.reduce((s, p) => s + (p.memory_bytes ?? 0), 0),
  }), [baseProcesses])


  // @group BusinessLogic > Ports : Refresh while apps are active so a newly
  // started dev server gets its web button as soon as it begins listening.
  const [portData, setPortData] = useState<PortScanEntry[]>([])
  const loadPorts = useCallback(() => {
    api.getPorts().then(data => setPortData(data.ports ?? [])).catch(() => {})
  }, [])
  const hasActiveProcesses = processes.some(p =>
    p.status === 'starting' || p.status === 'running' || p.status === 'watching'
  )
  useEffect(() => {
    loadPorts()
    if (!hasActiveProcesses) return
    const timer = window.setInterval(loadPorts, 5_000)
    return () => window.clearInterval(timer)
  }, [hasActiveProcesses, loadPorts])

  const portMap = useMemo(() => {
    const managedPids = new Set(displayedProcesses.map(p => p.pid).filter((pid): pid is number => pid != null))
    return listeningPortsByManagedPid(portData, managedPids)
  }, [portData, displayedProcesses])

  // @group BusinessLogic > Namespace groups : For table view section headers + card ns headers
  const [collapsed, setCollapsed]         = useState<Set<string>>(new Set())
  const [envModalProcess, setEnvModalProcess] = useState<ProcessInfo | null>(null)
  const [notifProcess, setNotifProcess]       = useState<ProcessInfo | null>(null)
  const [notifNs, setNotifNs]                 = useState<string | null>(null)
  const navigate = useNavigate()
  const { dialogState, confirm, danger, handleConfirm, handleCancel } = useDialog()

  const groups = useMemo(() => {
    const map = new Map<string, ProcessInfo[]>()
    for (const p of displayedProcesses) {
      const ns = p.namespace || 'default'
      if (!map.has(ns)) map.set(ns, [])
      map.get(ns)!.push(p)
    }
    return map
  }, [displayedProcesses])

  const sortedNs = useMemo(() =>
    [...groups.keys()].sort((a, b) => a === 'default' ? -1 : b === 'default' ? 1 : a.localeCompare(b)),
  [groups])

  function toggleNs(ns: string) {
    setCollapsed(prev => { const next = new Set(prev); if (next.has(ns)) next.delete(ns); else next.add(ns); return next })
  }

  async function startAll(ns: string)   { await api.startNamespace(ns).catch(() => {}); setTimeout(reload, 300) }
  async function restartAll(ns: string) { await api.restartNamespace(ns).catch(() => {}); setTimeout(reload, 400) }
  async function stopAll(ns: string) {
    const targets = (groups.get(ns) ?? []).filter(p => p.status === 'running' || p.status === 'watching')
    const ok = await confirm(`停止“${ns}”中的全部进程？`, `将停止 ${targets.length} 个运行中的进程。`)
    if (!ok) return
    await api.stopNamespace(ns).catch(() => {})
    setTimeout(reload, 400)
  }

  const hasActiveFilter = search || statusFilter !== 'all' || nsFilter !== 'all'
  function clearFilters() { setSearch(''); setStatusFilter('all'); setNsFilter('all') }

  const sharedRowProps = {
    reload,
    confirmDelete: settings.confirmBeforeDelete,
    onConfirm: confirm,
    onDanger: danger,
    onOpenDetail: (p: ProcessInfo) => navigate(`/processes/${p.id}`),
    onEdit: (p: ProcessInfo) => navigate(`/edit/${p.id}`),
    onOpenEnv: setEnvModalProcess,
    onOpenNotif: setNotifProcess,
    onOpenTerminal: onOpenTerminal ?? (() => {}),
    server: getActiveServer(),
    visibleRowActions: settings.visibleRowActions,
  }

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%' }}>
      <Dialog
        open={dialogState.open} title={dialogState.title} message={dialogState.message}
        variant={dialogState.variant} confirmLabel={dialogState.confirmLabel} cancelLabel={dialogState.cancelLabel}
        onConfirm={handleConfirm} onCancel={handleCancel}
      />
      {notifProcess && <ProcessNotifModal process={notifProcess} onClose={() => setNotifProcess(null)} />}
      {notifNs     && <NsNotifModal ns={notifNs} onClose={() => setNotifNs(null)} />}

      <div style={{ display: 'flex', flex: 1, minHeight: 0, overflow: 'hidden' }}>
        {/* Left column: header + scrollable content */}
        <div style={{ flex: 1, display: 'flex', flexDirection: 'column', minWidth: 0, overflow: 'hidden' }}>

          {/* ── Header ── */}
          <div style={{ padding: '14px 20px 12px', borderBottom: '1px solid var(--color-border)', flexShrink: 0, display: 'flex', flexDirection: 'column', gap: 10 }}>

            {/* Row 1: Title + controls */}
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                {namespaceFilter && (
                  <button onClick={() => navigate('/processes')} style={smallBtnStyle}>← 全部</button>
                )}
                <h2 style={{ fontSize: 15, fontWeight: 700, letterSpacing: '-0.2px' }}>
                  {namespaceFilter
                    ? <><span style={{ color: 'var(--color-muted-foreground)', fontWeight: 400 }}>ns / </span>{namespaceFilter}</>
                    : '进程'}
                </h2>
                <span style={{ fontSize: 11, color: 'var(--color-muted-foreground)', background: 'var(--color-muted)', padding: '1px 7px', borderRadius: 8 }}>
                  {stats.total}
                </span>
              </div>
              <div style={{ display: 'flex', gap: 6, alignItems: 'center' }}>
                {/* View toggle */}
                <div style={{ display: 'flex', border: '1px solid var(--color-border)', borderRadius: 6, overflow: 'hidden' }}>
                  <button onClick={() => setView('table')} title="表格视图"
                    style={{ padding: '5px 9px', background: viewMode === 'table' ? 'var(--color-primary)' : 'transparent', border: 'none', cursor: 'pointer', color: viewMode === 'table' ? '#fff' : 'var(--color-muted-foreground)', display: 'flex', alignItems: 'center', transition: 'all 0.12s' }}>
                    <List size={13} />
                  </button>
                  <button onClick={() => setView('cards')} title="卡片视图"
                    style={{ padding: '5px 9px', background: viewMode === 'cards' ? 'var(--color-primary)' : 'transparent', border: 'none', cursor: 'pointer', color: viewMode === 'cards' ? '#fff' : 'var(--color-muted-foreground)', display: 'flex', alignItems: 'center', transition: 'all 0.12s' }}>
                    <LayoutGrid size={13} />
                  </button>
                </div>
                <button onClick={() => { reload(); loadPorts() }} style={smallBtnStyle}>↻ 刷新</button>
              </div>
            </div>


           
            {/* Row 4: Search */}
            <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
              <div style={{ flex: 1, position: 'relative' }}>
                <span style={{ position: 'absolute', left: 10, top: '50%', transform: 'translateY(-50%)', color: 'var(--color-muted-foreground)', fontSize: 12, pointerEvents: 'none' }}>⌕</span>
                <input
                  value={search}
                  onChange={e => setSearch(e.target.value)}
                  placeholder="按名称或脚本搜索…"
                  style={{
                    width: '100%', padding: '6px 10px 6px 28px', fontSize: 12, boxSizing: 'border-box',
                    background: 'var(--color-secondary)', border: '1px solid var(--color-border)',
                    borderRadius: 6, color: 'var(--color-foreground)', outline: 'none',
                  }}
                />
              </div>
              {hasActiveFilter && (
                <button onClick={clearFilters} style={{ ...smallBtnStyle, padding: '5px 10px', fontSize: 11, color: 'var(--color-muted-foreground)' }}>✕ 清除筛选</button>
              )}
            </div>
          </div>

          {/* ── Content ── */}
          {displayedProcesses.length === 0 ? (
            <div style={{ padding: 32, color: 'var(--color-muted-foreground)' }}>
              {namespaceFilter ? `命名空间“${namespaceFilter}”中没有进程。` : hasActiveFilter ? '没有符合筛选条件的进程。' : '尚未注册进程。'}
            </div>
          ) : viewMode === 'cards' ? (
            /* Card grid view */
            <div style={{ flex: 1, overflow: 'auto', padding: '16px 20px' }}>
              {sortedNs.map(ns => {
                const procs = groups.get(ns)!
                const allActive   = procs.every(p => p.status === 'running' || p.status === 'watching')
                const allInactive = procs.every(p => p.status !== 'running' && p.status !== 'watching')
                const hasActive   = procs.some(p => p.status === 'running' || p.status === 'watching' || p.status === 'sleeping')
                return (
                  <div key={ns} style={{ marginBottom: 24 }}>
                    {/* Namespace header (only when not filtered to one ns) */}
                    {(sortedNs.length > 1 || !namespaceFilter) && (
                      <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 10 }}>
                        <span style={{ fontSize: 12, fontWeight: 700, letterSpacing: '0.04em' }}>{ns}</span>
                        <span style={{ fontSize: 11, color: 'var(--color-muted-foreground)', background: 'var(--color-muted)', padding: '1px 6px', borderRadius: 8 }}>{procs.length}</span>
                        <div style={{ flex: 1, height: 1, background: 'var(--color-border)' }} />
                        <span onClick={e => e.stopPropagation()} style={{ display: 'flex', gap: 4 }}>
                          {!allActive  && <NsBtn label="▶ 启动"   onClick={() => startAll(ns)} />}
                          {hasActive   && <NsBtn label="↺ 重启" onClick={() => restartAll(ns)} />}
                          {!allInactive && <NsBtn label="■ 停止"   onClick={() => stopAll(ns)} danger />}
                          <button onClick={() => setNotifNs(ns)} title="命名空间通知"
                            style={{ padding: '2px 5px', background: 'transparent', border: '1px solid var(--color-border)', borderRadius: 4, cursor: 'pointer', display: 'flex', alignItems: 'center' }}>
                            <Bell size={11} style={{ color: '#a78bfa' }} />
                          </button>
                        </span>
                      </div>
                    )}
                    <div style={{
                      display: 'grid',
                      gridTemplateColumns: 'repeat(auto-fill, minmax(290px, 1fr))',
                      gap: 12,
                    }}>
                      {procs.map(p => (
                        <ProcessCard
                          key={p.id} p={p}
                          ports={p.pid != null ? (portMap.get(p.pid) ?? []) : []}
                          {...sharedRowProps}
                        />
                      ))}
                    </div>
                  </div>
                )
              })}
            </div>
          ) : (
            /* Table view */
            <div style={{ flex: 1, overflow: 'auto' }}>
              <table style={{ width: '100%', borderCollapse: 'collapse', fontSize: 13 }}>
                <thead>
                  <tr style={{ background: 'var(--color-card)', borderBottom: '1px solid var(--color-border)' }}>
                    <Th>ID</Th>
                    <SortTh col="name"     active={sortCol} dir={sortDir} onSort={handleSort}>名称</SortTh>
                    <SortTh col="status"   active={sortCol} dir={sortDir} onSort={handleSort}>状态</SortTh>
                    <Th>PID</Th>
                    <SortTh col="uptime"   active={sortCol} dir={sortDir} onSort={handleSort}>运行时长</SortTh>
                    <SortTh col="cpu"      active={sortCol} dir={sortDir} onSort={handleSort}>CPU</SortTh>
                    <SortTh col="memory"   active={sortCol} dir={sortDir} onSort={handleSort}>内存</SortTh>
                    <SortTh col="restarts" active={sortCol} dir={sortDir} onSort={handleSort}>重启次数</SortTh>
                    <Th>模式</Th>
                    <Th>下次运行</Th>
                    <Th>上次运行</Th>
                    <Th>操作</Th>
                  </tr>
                </thead>
                <tbody>
                  {sortedNs.map(ns => {
                    const procs = groups.get(ns)!
                    const isCollapsed = collapsed.has(ns)
                    const allActive   = procs.every(p => p.status === 'running' || p.status === 'watching')
                    const allInactive = procs.every(p => p.status !== 'running' && p.status !== 'watching')
                    const hasActive   = procs.some(p => p.status === 'running' || p.status === 'watching' || p.status === 'sleeping')
                    return [
                      <tr key={`ns-${ns}`} onClick={() => toggleNs(ns)}
                        style={{ background: 'var(--color-muted)', cursor: 'pointer', userSelect: 'none' }}>
                        <td colSpan={12} style={{ padding: '6px 12px' }}>
                          <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                            <span style={{ fontSize: 10, color: 'var(--color-muted-foreground)' }}>{isCollapsed ? '▶' : '▼'}</span>
                            <span style={{ fontWeight: 600, fontSize: 12 }}>{ns}</span>
                            <span style={{ fontSize: 11, color: 'var(--color-muted-foreground)' }}>{procs.length} 个进程</span>
                            <span onClick={e => e.stopPropagation()} style={{ marginLeft: 'auto', display: 'flex', gap: 5, alignItems: 'center' }}>
                              {!allActive  && <NsBtn label="▶ 全部启动"   onClick={() => startAll(ns)} />}
                              {hasActive   && <NsBtn label="↺ 全部重启" onClick={() => restartAll(ns)} />}
                              {!allInactive && <NsBtn label="■ 全部停止"   onClick={() => stopAll(ns)} danger />}
                              <button onClick={() => setNotifNs(ns)} title="命名空间通知"
                                style={{ padding: '2px 5px', background: 'transparent', border: '1px solid var(--color-border)', borderRadius: 4, cursor: 'pointer', display: 'flex', alignItems: 'center' }}>
                                <Bell size={11} style={{ color: '#a78bfa' }} />
                              </button>
                            </span>
                          </div>
                        </td>
                      </tr>,
                      ...(!isCollapsed ? procs.map(p => (
                        <ProcessRow
                          key={p.id} p={p}
                          ports={p.pid != null ? (portMap.get(p.pid) ?? []) : []}
                          {...sharedRowProps}
                        />
                      )) : []),
                    ]
                  })}
                </tbody>
              </table>
            </div>
          )}
        </div>

        {/* Right column: .env panel */}
        {envModalProcess && (
          <div style={{ width: 400, flexShrink: 0, borderLeft: '1px solid var(--color-border)', display: 'flex', flexDirection: 'column' }}>
            <EnvFilePanel
              processId={envModalProcess.id}
              processName={envModalProcess.name}
              onClose={() => setEnvModalProcess(null)}
              onRestart={reload}
            />
          </div>
        )}
      </div>
    </div>
  )
}

// @group BusinessLogic > ProcessCard : Card view — colored border, mini resource bars, actions
function ProcessCard({ p, reload, confirmDelete, onConfirm, onDanger, onOpenDetail, onEdit, onOpenEnv, onOpenNotif, onOpenTerminal, ports, server, visibleRowActions }: RowProps) {
  const navigate = useNavigate()
  const isActive = p.status === 'running' || p.status === 'sleeping' || p.status === 'watching'
  const color    = statusColor(p.status)
  const hasNotify = !!p.notify?.webhook?.enabled || !!p.notify?.slack?.enabled || !!p.notify?.teams?.enabled || !!p.notify?.discord?.enabled

  const cpuPct = Math.min(p.cpu_percent ?? 0, 100)
  const memPct = Math.min(((p.memory_bytes ?? 0) / (512 * 1024 * 1024)) * 100, 100)

  async function doStop() {
    const ok = await onConfirm(`停止“${p.name}”？`, '进程将停止，之后可以重新启动。')
    if (!ok) return
    await api.stopProcess(p.id).catch(() => {})
    setTimeout(reload, 300)
  }
  async function doStart()   { await api.startStopped(p.id).catch(() => {}); setTimeout(reload, 300) }
  async function doRestart() { await api.restartProcess(p.id).catch(() => {}); reload() }
  async function doDelete() {
    if (confirmDelete) {
      const ok = await onDanger(`删除“${p.name}”？`, '这将停止并永久删除该进程。', '删除')
      if (!ok) return
    }
    await api.deleteProcess(p.id).catch(() => {})
    setTimeout(reload, 300)
  }
  async function doClone() { await api.cloneProcess(p.id).catch(() => {}); setTimeout(reload, 400) }
  async function doToggleEnabled() { await api.setProcessEnabled(p.id, !p.enabled).catch(() => {}); setTimeout(reload, 200) }

  const [hovered, setHovered] = useState(false)
  const isDisabled = p.enabled === false

  return (
    <div
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      style={{
        background: isDisabled ? 'color-mix(in srgb, var(--color-muted-foreground) 5%, var(--color-card))' : 'var(--color-card)',
        border: `1px solid ${hovered ? (isDisabled ? 'var(--color-muted-foreground)' : color + '60') : 'var(--color-border)'}`,
        borderLeft: `4px solid ${isDisabled ? 'var(--color-muted-foreground)' : color}`,
        borderRadius: 8,
        padding: '12px 14px',
        display: 'flex',
        flexDirection: 'column',
        gap: 8,
        transition: 'border-color 0.15s, box-shadow 0.15s',
        boxShadow: hovered && !isDisabled ? `0 2px 12px ${color}18` : 'none',
        opacity: isDisabled ? 0.7 : 1,
      }}
    >
      {/* Name + status badge */}
      <div style={{ display: 'flex', alignItems: 'flex-start', justifyContent: 'space-between', gap: 8 }}>
        <div style={{ display: 'flex', flexDirection: 'column', gap: 2, minWidth: 0 }}>
          <strong
            style={{ fontSize: 14, cursor: 'pointer', wordBreak: 'break-word', lineHeight: 1.3 }}
            onClick={() => onOpenDetail(p)}
          >{p.name}</strong>
          {p.git_branch && (
            <span style={{ display: 'flex', alignItems: 'center', gap: 3, fontSize: 10, color: 'var(--color-muted-foreground)' }}>
              <GitBranch size={10} />
              {p.git_branch}
            </span>
          )}
        </div>
        <span style={{
          fontSize: 10, fontWeight: 700, whiteSpace: 'nowrap', flexShrink: 0,
          padding: '2px 8px', borderRadius: 10,
          background: color + '22', color,
        }}>● {processStatusLabel(p.status)}</span>
      </div>

      {/* Badges: namespace + mode */}
      <div style={{ display: 'flex', gap: 4, flexWrap: 'wrap' }}>
        <span style={{ fontSize: 10, padding: '1px 6px', borderRadius: 8, background: 'var(--color-muted)', color: 'var(--color-muted-foreground)' }}>
          {p.namespace || 'default'}
        </span>
        {p.cron && (
          <span style={{ fontSize: 10, padding: '1px 6px', borderRadius: 8, background: 'rgba(79,156,249,0.15)', color: 'var(--color-status-sleeping)' }}
            title={p.cron}>定时任务</span>
        )}
        {p.watch && !p.cron && (
          <span style={{ fontSize: 10, padding: '1px 6px', borderRadius: 8, background: 'rgba(250,204,21,0.15)', color: '#fbbf24' }}>监视</span>
        )}
      </div>

      {/* CPU + Memory mini bars */}
      {isActive && (
        <div style={{ display: 'flex', flexDirection: 'column', gap: 5 }}>
          <MiniBar label="CPU" value={cpuPct} color="#60a5fa" text={formatCpu(p.cpu_percent ?? 0)} />
          <MiniBar label="内存" value={memPct} color="#a78bfa" text={formatBytes(p.memory_bytes ?? 0)} />
        </div>
      )}

      {/* Uptime + restarts + PID */}
      <div style={{ display: 'flex', gap: 10, fontSize: 11, color: 'var(--color-muted-foreground)', flexWrap: 'wrap' }}>
        {p.uptime_secs != null && <span>⏱ {formatUptime(p.uptime_secs)}</span>}
        <span style={{ color: p.restart_count > 5 ? '#f87171' : undefined }}>↺ {p.restart_count}</span>
        {p.pid != null && <span>PID {p.pid}</span>}
      </div>

      {/* Port badges */}
      {ports.length > 0 && (
        <div style={{ display: 'flex', gap: 3, flexWrap: 'wrap' }}>
          {ports.slice(0, 6).map(port => (
            <span key={port} style={{
              fontSize: 10, fontWeight: 700, padding: '1px 5px', borderRadius: 3,
              background: 'color-mix(in srgb, var(--color-primary) 18%, transparent)',
              color: 'var(--color-primary)',
            }}>:{port}</span>
          ))}
          {ports.length > 6 && (
            <span style={{ fontSize: 10, color: 'var(--color-muted-foreground)' }}>+{ports.length - 6}</span>
          )}
        </div>
      )}

      {/* Action buttons — primary always visible, secondary split by visibleRowActions setting */}
      {(() => {
        const secondary = [
          { key: 'logs',     label: '日志',     icon: ScrollText,    onClick: () => navigate(`/processes/${p.id}`),                   color: '#60a5fa' },
          { key: 'edit',     label: '编辑',     icon: Pencil,        onClick: () => onEdit(p),                                        color: '#34d399' },
          { key: 'terminal', label: '终端', icon: SquareTerminal, onClick: () => p.cwd && onOpenTerminal(p.cwd, p.name),           color: '#22d3ee', disabled: !p.cwd },
          { key: 'env',      label: '.env',     icon: FileKey,       onClick: () => onOpenEnv(p),                                     color: '#fbbf24' },
          { key: 'enable',   label: isDisabled ? '启用' : '停用', icon: isDisabled ? Power : PowerOff, onClick: doToggleEnabled, color: isDisabled ? '#4ade80' : 'var(--color-muted-foreground)' },
          { key: 'notify',   label: '通知',   icon: Bell,          onClick: () => onOpenNotif(p),                                   color: '#a78bfa', badge: hasNotify ? '●' : undefined },
          { key: 'clone',    label: '克隆',    icon: Copy,          onClick: doClone,                                                color: '#94a3b8' },
          { key: 'delete',   label: '删除',   icon: Trash2,        onClick: doDelete,                                               danger: true },
        ]
        const inline   = secondary.filter(a => visibleRowActions.includes(a.key))
        const overflow = secondary.filter(a => !visibleRowActions.includes(a.key))
        return (
          <div style={{ display: 'flex', gap: 4, flexWrap: 'wrap', paddingTop: 2, borderTop: '1px solid var(--color-border)' }}>
            {isActive
              ? <><ActionBtn label="重启" icon={RotateCcw} onClick={doRestart} color="#fb923c" /><ActionBtn label="停止" icon={Square} onClick={doStop} color="#f87171" /></>
              : <ActionBtn label="启动" icon={Play} onClick={isDisabled ? undefined : doStart} color={isDisabled ? 'var(--color-muted-foreground)' : '#4ade80'} disabled={isDisabled} title={isDisabled ? '进程已停用，请先启用' : '启动'} />
            }
            <WebPortButton ports={ports} server={server} />
            {inline.map(a => <ActionBtn key={a.key} label={a.label} icon={a.icon} onClick={a.onClick} color={a.color} danger={a.danger} badge={a.badge} />)}
            <RowOverflowMenu actions={overflow} />
          </div>
        )
      })()}
    </div>
  )
}

// @group BusinessLogic > ProcessRow : Table view row
interface RowProps {
  p: ProcessInfo
  reload: () => void
  confirmDelete: boolean
  onConfirm: (title: string, message?: string) => Promise<boolean>
  onDanger: (title: string, message?: string, confirmLabel?: string) => Promise<boolean>
  onOpenDetail: (p: ProcessInfo) => void
  onEdit: (p: ProcessInfo) => void
  onOpenEnv: (p: ProcessInfo) => void
  onOpenNotif: (p: ProcessInfo) => void
  onOpenTerminal: (cwd: string, name?: string) => void
  ports: number[]
  server: RemoteServer
  visibleRowActions: string[]
}

function ProcessRow({ p, reload, confirmDelete, onConfirm, onDanger, onOpenDetail, onEdit, onOpenEnv, onOpenNotif, onOpenTerminal, ports, server, visibleRowActions }: RowProps) {
  const navigate = useNavigate()
  const isActive  = p.status === 'running' || p.status === 'sleeping' || p.status === 'watching'
  const hasNotify = !!p.notify?.webhook?.enabled || !!p.notify?.slack?.enabled || !!p.notify?.teams?.enabled || !!p.notify?.discord?.enabled

  const modeCell = p.cron
    ? <span style={{ display: 'inline-block', padding: '1px 7px', borderRadius: 4, fontSize: 11, fontWeight: 600, background: 'rgba(79,156,249,0.15)', color: 'var(--color-status-sleeping)', cursor: 'default' }} title={p.cron}>定时任务</span>
    : p.watch ? '监视' : '-'

  async function doStop() {
    const ok = await onConfirm(`停止“${p.name}”？`, '进程将停止，之后可以重新启动。')
    if (!ok) return
    await api.stopProcess(p.id).catch(() => {})
    setTimeout(reload, 300)
  }
  async function doStart()   { await api.startStopped(p.id).catch(() => {}); setTimeout(reload, 300) }
  async function doRestart() { await api.restartProcess(p.id).catch(() => {}); reload() }
  async function doDelete() {
    if (confirmDelete) {
      const ok = await onDanger(`删除“${p.name}”？`, '这将停止并永久删除该进程。', '删除')
      if (!ok) return
    }
    await api.deleteProcess(p.id).catch(() => {})
    setTimeout(reload, 300)
  }
  async function doClone() { await api.cloneProcess(p.id).catch(() => {}); setTimeout(reload, 400) }
  async function doToggleEnabled() { await api.setProcessEnabled(p.id, !p.enabled).catch(() => {}); setTimeout(reload, 200) }

  const isDisabled = p.enabled === false

  return (
    <tr style={{ borderBottom: '1px solid var(--color-border)', background: isDisabled ? 'color-mix(in srgb, var(--color-muted-foreground) 5%, transparent)' : 'transparent' }}
      onMouseEnter={e => (e.currentTarget.style.background = isDisabled ? 'color-mix(in srgb, var(--color-muted-foreground) 9%, transparent)' : 'var(--color-accent)')}
      onMouseLeave={e => (e.currentTarget.style.background = isDisabled ? 'color-mix(in srgb, var(--color-muted-foreground) 5%, transparent)' : 'transparent')}
    >
      <Td>
        <code style={{ fontSize: 11, color: 'var(--color-muted-foreground)', cursor: 'pointer' }}
          title={p.id} onClick={() => onOpenDetail(p)}>{p.id.slice(0, 8)}</code>
      </Td>
      <Td>
        <div style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
          <strong style={{ cursor: 'pointer' }} onClick={() => onOpenDetail(p)}>{p.name}</strong>
          {p.git_branch && (
            <span style={{ display: 'flex', alignItems: 'center', gap: 3, fontSize: 10, color: 'var(--color-muted-foreground)' }}>
              <GitBranch size={10} />
              {p.git_branch}
            </span>
          )}
        </div>
      </Td>
      <Td>
        <span style={{ color: statusColor(p.status), display: 'flex', alignItems: 'center', gap: 4, fontSize: 12 }}>
          ● {processStatusLabel(p.status)}
        </span>
      </Td>
      <Td style={{ whiteSpace: 'normal', verticalAlign: 'top' }}>
        <div style={{ display: 'flex', flexDirection: 'column', gap: 3 }}>
          <span style={{ fontFamily: 'monospace', fontSize: 12 }}>{p.pid ?? '-'}</span>
          {ports.length > 0 ? (
            <div style={{ display: 'flex', gap: 2, flexWrap: 'wrap' }}>
              {ports.slice(0, 5).map(port => (
                <span key={port} title={`端口 ${port}`} style={{
                  fontSize: 9, fontWeight: 700, lineHeight: '14px', padding: '1px 4px', borderRadius: 3,
                  background: 'color-mix(in srgb, var(--color-primary) 18%, transparent)', color: 'var(--color-primary)', cursor: 'default',
                }}>:{port}</span>
              ))}
              {ports.length > 5 && (
                <span title={ports.slice(5).map(p => `:${p}`).join(' ')} style={{ fontSize: 9, lineHeight: '14px', color: 'var(--color-muted-foreground)', cursor: 'default' }}>+{ports.length - 5}</span>
              )}
            </div>
          ) : (isActive && p.pid != null) ? (
            <span title="未找到监听中的 TCP 端口" style={{ fontSize: 9, color: 'var(--color-muted-foreground)', opacity: 0.5, cursor: 'default', fontStyle: 'italic' }}>无端口</span>
          ) : null}
        </div>
      </Td>
      <Td>{p.uptime_secs != null ? formatUptime(p.uptime_secs) : '-'}</Td>
      <Td style={{ color: 'var(--color-muted-foreground)' }}>{p.cpu_percent != null ? formatCpu(p.cpu_percent) : '-'}</Td>
      <Td style={{ color: 'var(--color-muted-foreground)' }}>{p.memory_bytes != null ? formatBytes(p.memory_bytes) : '-'}</Td>
      <Td>{p.restart_count}</Td>
      <Td>{modeCell}</Td>
      <Td style={{ color: 'var(--color-muted-foreground)' }}>{formatNextRun(p.cron_next_run)}</Td>
      <Td style={{ color: 'var(--color-muted-foreground)' }} title={p.stopped_at ?? p.started_at ?? ''}>{formatLastRun(p)}</Td>
      <Td>
        {(() => {
          const secondary = [
            { key: 'logs',     label: '日志',     icon: ScrollText,    onClick: () => navigate(`/processes/${p.id}`),                   color: '#60a5fa' },
            { key: 'edit',     label: '编辑',     icon: Pencil,        onClick: () => onEdit(p),                                        color: '#34d399' },
            { key: 'terminal', label: '终端', icon: SquareTerminal, onClick: () => p.cwd && onOpenTerminal(p.cwd, p.name),           color: '#22d3ee', disabled: !p.cwd },
            { key: 'env',      label: '.env',     icon: FileKey,       onClick: () => onOpenEnv(p),                                     color: '#fbbf24' },
            { key: 'enable',   label: isDisabled ? '启用' : '停用', icon: isDisabled ? Power : PowerOff, onClick: doToggleEnabled, color: isDisabled ? '#4ade80' : 'var(--color-muted-foreground)' },
            { key: 'notify',   label: '通知',   icon: Bell,          onClick: () => onOpenNotif(p),                                   color: '#a78bfa', badge: hasNotify ? '●' : undefined },
            { key: 'clone',    label: '克隆',    icon: Copy,          onClick: doClone,                                                color: '#94a3b8' },
            { key: 'delete',   label: '删除',   icon: Trash2,        onClick: doDelete,                                               danger: true },
          ]
          const inline   = secondary.filter(a => visibleRowActions.includes(a.key))
          const overflow = secondary.filter(a => !visibleRowActions.includes(a.key))
          return (
            <div style={{ display: 'flex', gap: 3, flexWrap: 'nowrap' }}>
              {isActive
                ? <><ActionBtn label="重启" icon={RotateCcw} onClick={doRestart} color="#fb923c" /><ActionBtn label="停止" icon={Square} onClick={doStop} color="#f87171" /></>
                : <ActionBtn label="启动" icon={Play} onClick={isDisabled ? undefined : doStart} color={isDisabled ? 'var(--color-muted-foreground)' : '#4ade80'} disabled={isDisabled} title={isDisabled ? '进程已停用，请先启用' : '启动'} />
              }
              <WebPortButton ports={ports} server={server} />
              {inline.map(a => <ActionBtn key={a.key} label={a.label} icon={a.icon} onClick={a.onClick} color={a.color} danger={a.danger} badge={a.badge} />)}
              <RowOverflowMenu actions={overflow} />
            </div>
          )
        })()}
      </Td>
    </tr>
  )
}


// @group Utilities > MiniBar : Horizontal resource bar for card view
function MiniBar({ label, value, color, text }: { label: string; value: number; color: string; text: string }) {
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
      <span style={{ width: 26, fontSize: 10, color: 'var(--color-muted-foreground)', flexShrink: 0 }}>{label}</span>
      <div style={{ flex: 1, height: 3, background: 'var(--color-border)', borderRadius: 2, overflow: 'hidden' }}>
        <div style={{ width: `${Math.max(value, 0)}%`, height: '100%', background: color, borderRadius: 2, transition: 'width 0.3s' }} />
      </div>
      <span style={{ width: 54, fontSize: 10, color: 'var(--color-muted-foreground)', textAlign: 'right', flexShrink: 0 }}>{text}</span>
    </div>
  )
}

// @group Utilities > UI helpers
function Th({ children }: { children: React.ReactNode }) {
  return (
    <th style={{ padding: '8px 12px', textAlign: 'left', fontWeight: 600, fontSize: 11, color: 'var(--color-muted-foreground)', letterSpacing: '0.04em', whiteSpace: 'nowrap' }}>
      {children}
    </th>
  )
}

function SortTh({ children, col, active, dir, onSort }: {
  children: React.ReactNode; col: SortCol; active: SortCol; dir: SortDir; onSort: (col: SortCol) => void
}) {
  const isActive = active === col
  return (
    <th onClick={() => onSort(col)} style={{
      padding: '8px 12px', textAlign: 'left', fontWeight: 600, fontSize: 11, letterSpacing: '0.04em',
      color: isActive ? 'var(--color-foreground)' : 'var(--color-muted-foreground)',
      whiteSpace: 'nowrap', cursor: 'pointer', userSelect: 'none',
    }}>
      <span style={{ display: 'inline-flex', alignItems: 'center', gap: 3 }}>
        {children}
        {isActive
          ? dir === 'asc' ? <ChevronUp size={11} /> : <ChevronDown size={11} />
          : <ChevronsUpDown size={11} style={{ opacity: 0.35 }} />
        }
      </span>
    </th>
  )
}

function Td({ children, style, title }: { children?: React.ReactNode; style?: React.CSSProperties; title?: string }) {
  return <td title={title} style={{ padding: '7px 12px', whiteSpace: 'nowrap', ...style }}>{children}</td>
}

// @group BusinessLogic > RowOverflowMenu : ⋯ dropdown for secondary process actions
function RowOverflowMenu({ actions }: { actions: { label: string; icon: React.ElementType; onClick: () => void; danger?: boolean; color?: string; badge?: string }[] }) {
  const [open, setOpen] = useState(false)
  const ref = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (!open) return
    const handler = (e: MouseEvent) => { if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false) }
    document.addEventListener('mousedown', handler)
    return () => document.removeEventListener('mousedown', handler)
  }, [open])

  if (actions.length === 0) return null

  return (
    <div ref={ref} style={{ position: 'relative' }}>
      <button
        title="更多操作"
        onClick={() => setOpen(v => !v)}
        style={{
          position: 'relative', padding: 0, width: 26, height: 26,
          display: 'flex', alignItems: 'center', justifyContent: 'center',
          background: open ? 'var(--color-accent)' : 'var(--color-secondary)',
          border: '1px solid var(--color-border)',
          borderRadius: 4, cursor: 'pointer', flexShrink: 0,
          color: 'var(--color-muted-foreground)',
          fontSize: 13, letterSpacing: 1,
        }}
      >
        ···
      </button>
      {open && (
        <div style={{
          position: 'absolute', top: '110%', right: 0,
          background: 'var(--color-card)', border: '1px solid var(--color-border)',
          borderRadius: 6, padding: '4px 0', minWidth: 150, zIndex: 500,
          boxShadow: '0 4px 16px rgba(0,0,0,0.4)',
        }}>
          {actions.map(a => {
            const Icon = a.icon
            const iconColor = a.danger ? 'var(--color-destructive)' : (a.color ?? 'var(--color-muted-foreground)')
            return (
              <button
                key={a.label}
                onClick={() => { a.onClick(); setOpen(false) }}
                style={{
                  display: 'flex', alignItems: 'center', gap: 8,
                  padding: '6px 12px', width: '100%', textAlign: 'left',
                  background: 'transparent', border: 'none', cursor: 'pointer',
                  fontSize: 12, fontWeight: 500, color: iconColor, position: 'relative',
                }}
                onMouseEnter={e => { e.currentTarget.style.background = 'var(--color-accent)' }}
                onMouseLeave={e => { e.currentTarget.style.background = 'transparent' }}
              >
                <Icon size={13} strokeWidth={1.75} color={iconColor} />
                {a.label}
                {a.badge && <span style={{ marginLeft: 'auto', fontSize: 8, color: iconColor }}>{a.badge}</span>}
              </button>
            )
          })}
        </div>
      )}
    </div>
  )
}

function ActionBtn({ label, icon: Icon, onClick, danger, color, badge, disabled, title }: {
  label: string; icon: React.ElementType; onClick?: () => void; danger?: boolean; color?: string; badge?: string; disabled?: boolean; title?: string
}) {
  const iconColor = disabled ? 'var(--color-muted-foreground)' : danger ? 'var(--color-destructive)' : (color ?? 'var(--color-muted-foreground)')
  const style: React.CSSProperties = {
    position: 'relative', padding: 0, width: 26, height: 26,
    display: 'flex', alignItems: 'center', justifyContent: 'center',
    background: disabled ? 'color-mix(in srgb, var(--color-secondary) 60%, transparent)' : 'var(--color-secondary)',
    border: '1px solid var(--color-border)', textDecoration: 'none',
    borderRadius: 4, cursor: disabled ? 'not-allowed' : 'pointer', flexShrink: 0, color: iconColor,
    opacity: disabled ? 0.45 : 1,
  }
  const content = <>
      <Icon size={13} strokeWidth={1.75} />
      {badge && <span style={{ position: 'absolute', top: -3, right: -3, fontSize: 8, color: iconColor, lineHeight: 1 }}>{badge}</span>}
    </>
  return <button title={title ?? label} onClick={disabled ? undefined : onClick} disabled={disabled} style={style}>{content}</button>
}

function NsBtn({ label, onClick, danger }: { label: string; onClick: () => void; danger?: boolean }) {
  return (
    <button onClick={onClick} style={{
      padding: '2px 8px', fontSize: 11, fontWeight: 600, background: 'transparent',
      border: `1px solid ${danger ? 'var(--color-destructive)' : 'var(--color-border)'}`,
      borderRadius: 4, cursor: 'pointer',
      color: danger ? 'var(--color-destructive)' : 'var(--color-foreground)',
    }}>{label}</button>
  )
}

const smallBtnStyle: React.CSSProperties = {
  padding: '4px 10px', fontSize: 12, background: 'var(--color-secondary)',
  border: '1px solid var(--color-border)', borderRadius: 5, cursor: 'pointer',
  color: 'var(--color-foreground)',
}
