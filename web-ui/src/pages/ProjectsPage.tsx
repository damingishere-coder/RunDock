// @group BusinessLogic : Project-first list grouped into common and pending categories

import { useCallback, useMemo, useState } from 'react'
import { Link, useLocation } from 'react-router-dom'
import {
  ChevronDown,
  ChevronRight,
  MonitorUp,
  Pencil,
  Play,
  Power,
  RotateCcw,
  Square,
} from 'lucide-react'
import { api } from '@/lib/api'
import {
  PROJECT_CATEGORIES,
  projectActionError,
  projectStatusColor,
  projectStatusLabel,
  sortProjects,
} from '@/lib/projects'
import { isPortScanEntries, listeningPortsByManagedPid, type PortScanEntry } from '@/lib/processWeb'
import { resolveActiveServer } from '@/lib/servers'
import { formatBytes, processStatusLabel, statusColor } from '@/lib/utils'
import { useDialog } from '@/hooks/useDialog'
import { useSingleFlightPoll } from '@/hooks/useSingleFlightPoll'
import { Dialog } from '@/components/Dialog'
import { WebPortButton } from '@/components/WebPortButton'
import { DesktopLaunchButton } from '@/components/DesktopLaunchButton'
import type { ProjectInfo } from '@/types'

interface Props {
  projects: ProjectInfo[]
  error: string | null
  reload: () => void
}

type BusyAction = 'start' | 'stop' | 'restart' | 'enable' | 'disable' | 'save-note' | 'move'

export default function ProjectsPage({ projects, error, reload }: Props) {
  const { hash, search: locationSearch } = useLocation()
  const { dialogState, confirm, handleConfirm, handleCancel } = useDialog()
  const [search, setSearch] = useState(() => new URLSearchParams(locationSearch).get('q') ?? '')
  const [expanded, setExpanded] = useState<Set<string>>(() => new Set(hash ? [hash.slice(1)] : []))
  const [selectedProjectId, setSelectedProjectId] = useState<string | null>(() =>
    hash ? hash.slice(1) : null
  )
  const [editingNote, setEditingNote] = useState<string | null>(null)
  const [draft, setDraft] = useState('')
  const [busy, setBusy] = useState<Record<string, BusyAction | undefined>>({})
  const [feedback, setFeedback] = useState<{ kind: 'success' | 'error'; text: string } | null>(null)
  const [portData, setPortData] = useState<PortScanEntry[]>([])

  const visibleProjects = useMemo(() => {
    const query = search.trim().toLowerCase()
    const filtered = query
      ? projects.filter(
          project =>
            project.display_name.toLowerCase().includes(query) ||
            project.note.toLowerCase().includes(query) ||
            project.members.some(member => member.name.toLowerCase().includes(query))
        )
      : projects
    return sortProjects(filtered)
  }, [projects, search])

  const groups = useMemo(() => {
    const grouped = new Map<string, ProjectInfo[]>()
    for (const category of PROJECT_CATEGORIES) grouped.set(category, [])
    for (const project of visibleProjects) {
      const category = PROJECT_CATEGORIES.includes(
        project.category as (typeof PROJECT_CATEGORIES)[number]
      )
        ? project.category
        : '常用'
      grouped.get(category)!.push(project)
    }
    return [...grouped.entries()]
  }, [visibleProjects])

  const selectedProject = useMemo(
    () => projects.find(project => project.id === selectedProjectId) ?? visibleProjects[0] ?? null,
    [projects, selectedProjectId, visibleProjects]
  )

  const stats = useMemo(
    () => ({
      total: projects.length,
      running: projects.filter(
        project => project.status === 'running' || project.status === 'partial'
      ).length,
      stopped: projects.filter(project => project.status === 'stopped').length,
      disabled: projects.filter(project => project.status === 'disabled').length,
      desktop: projects.filter(project => project.status === 'desktop').length,
    }),
    [projects]
  )

  const loadPorts = useCallback(async (isCurrent: () => boolean, signal: AbortSignal) => {
    try {
      const data = await api.getPorts({ signal })
      if (isCurrent()) {
        if (!isPortScanEntries(data.ports)) throw new Error('端口扫描返回了无效数据')
        setPortData(data.ports)
      }
    } catch (loadError) {
      if (isCurrent()) {
        setFeedback({
          kind: 'error',
          text: loadError instanceof Error ? loadError.message : '端口扫描失败',
        })
      }
      throw loadError
    }
  }, [])

  const hasActiveProjects = projects.some(project => project.active_process_count > 0)
  useSingleFlightPoll(loadPorts, {
    intervalMs: 5_000,
    enabled: hasActiveProjects,
  })

  const portsByPid = useMemo(() => {
    const managedPids = projects.flatMap(project =>
      project.members.flatMap(member => (member.pid == null ? [] : [member.pid]))
    )
    return listeningPortsByManagedPid(portData, managedPids)
  }, [portData, projects])

  const projectPorts = useMemo(() => {
    const result = new Map<string, number[]>()

    for (const project of projects) {
      const ports = new Set<number>()
      for (const member of project.members) {
        if (member.pid == null) continue
        for (const port of portsByPid.get(member.pid) ?? []) ports.add(port)
      }
      result.set(
        project.id,
        [...ports].sort((a, b) => a - b)
      )
    }
    return result
  }, [portsByPid, projects])

  const activeServer = resolveActiveServer()

  function toggleProject(id: string) {
    setExpanded(current => {
      const next = new Set(current)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })
  }

  async function withBusy(id: string, action: BusyAction, task: () => Promise<string>) {
    setBusy(current => ({ ...current, [id]: action }))
    setFeedback(null)
    try {
      const message = await task()
      setFeedback({ kind: 'success', text: message })
      reload()
    } catch (actionError) {
      reload()
      setFeedback({
        kind: 'error',
        text: actionError instanceof Error ? actionError.message : '操作失败',
      })
    } finally {
      setBusy(current => ({ ...current, [id]: undefined }))
    }
  }

  async function runAction(project: ProjectInfo, action: 'start' | 'stop' | 'restart') {
    if (action === 'stop') {
      const ok = await confirm(
        `停止“${project.display_name}”？`,
        `将停止该项目的 ${project.process_count} 个技术组件。`
      )
      if (!ok) return
    }
    await withBusy(project.id, action, async () => {
      const response =
        action === 'start'
          ? await api.startProject(project.id)
          : action === 'stop'
            ? await api.stopProject(project.id)
            : await api.restartProject(project.id)
      const actionError = projectActionError(response)
      if (actionError) throw new Error(actionError)
      return action === 'start'
        ? `${project.display_name} 已启动`
        : action === 'stop'
          ? `${project.display_name} 已停止`
          : `${project.display_name} 已重启`
    })
  }

  async function setEnabled(project: ProjectInfo, enabled: boolean) {
    if (!enabled) {
      const ok = await confirm(
        `停用“${project.display_name}”？`,
        'RunDock 会先停止全部组件；停用后不会参与批量启动。'
      )
      if (!ok) return
    }
    await withBusy(project.id, enabled ? 'enable' : 'disable', async () => {
      let stoppedBeforeDisable = false
      if (!enabled && project.active_process_count > 0) {
        const response = await api.stopProject(project.id)
        const stopError = projectActionError(response)
        if (stopError) throw new Error(`停止失败，项目没有被停用：${stopError}`)
        stoppedBeforeDisable = true
      }
      try {
        await api.updateProject(project.id, { enabled })
      } catch (updateError) {
        if (stoppedBeforeDisable) {
          throw new Error(
            `项目组件已停止，但停用状态保存失败：${updateError instanceof Error ? updateError.message : '未知错误'}`
          )
        }
        throw updateError
      }
      return enabled
        ? `${project.display_name} 已启用，可以手动启动`
        : `${project.display_name} 已停止并停用`
    })
  }

  async function saveNote(project: ProjectInfo) {
    const value = draft.trim()
    await withBusy(project.id, 'save-note', async () => {
      await api.updateProject(project.id, { note: value })
      setEditingNote(null)
      return '项目备注已保存'
    })
  }

  async function moveProject(project: ProjectInfo, category: string) {
    if (category === project.category) return
    await withBusy(project.id, 'move', async () => {
      await api.updateProject(project.id, { category })
      return `${project.display_name} 已移动到“${category}”`
    })
  }

  return (
    <div className="rundock-projects-page">
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

      <header className="rundock-projects-header">
        <div className="rundock-projects-title-row">
          <h2 className="rundock-page-title">项目总览</h2>
          <span style={countPillStyle}>{stats.total}</span>
          <div style={{ flex: 1 }} />
          <input
            value={search}
            onChange={event => setSearch(event.target.value)}
            placeholder="搜索项目、备注或组件…"
            aria-label="搜索项目"
            className="rundock-project-search"
          />
        </div>
        <div className="rundock-stat-grid" aria-label="项目状态统计">
          <div className="rundock-stat-card">
            <span>全部项目</span>
            <strong>{stats.total}</strong>
          </div>
          <div className="rundock-stat-card">
            <span>正在运行</span>
            <strong style={{ color: 'var(--color-status-running)' }}>{stats.running}</strong>
          </div>
          <div className="rundock-stat-card">
            <span>已停止或停用</span>
            <strong>{stats.stopped + stats.disabled}</strong>
          </div>
          <div className="rundock-stat-card">
            <span>桌面软件</span>
            <strong style={{ color: 'var(--color-primary)' }}>{stats.desktop}</strong>
          </div>
        </div>
        {feedback && (
          <div
            role="status"
            style={{
              ...feedbackStyle,
              color:
                feedback.kind === 'error'
                  ? 'var(--color-destructive)'
                  : 'var(--color-status-running)',
            }}
          >
            {feedback.text}
          </div>
        )}
        {activeServer.error && (
          <div role="alert" style={{ ...feedbackStyle, color: 'var(--color-destructive)' }}>
            {activeServer.error}；请使用左侧服务器切换器重置为本地服务器。
          </div>
        )}
        {error && (
          <div style={{ ...feedbackStyle, color: 'var(--color-destructive)' }}>
            项目数据加载失败：{error}
          </div>
        )}
      </header>

      <div className="rundock-projects-body">
        <main className="rundock-projects-list">
          {groups.map(([category, categoryProjects]) => (
            <section key={category} className="rundock-project-group">
              <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 9 }}>
                <h3 style={{ margin: 0, fontSize: 13, letterSpacing: '0.04em' }}>{category}</h3>
                <span style={countPillStyle}>{categoryProjects.length}</span>
                {category === '待定' && (
                  <span style={{ fontSize: 11, color: 'var(--color-muted-foreground)' }}>
                    暂不维护，默认停用且不会批量启动
                  </span>
                )}
              </div>
              <div className="rundock-project-group-card">
                {categoryProjects.length === 0 ? (
                  <div
                    style={{ padding: 18, fontSize: 12, color: 'var(--color-muted-foreground)' }}
                  >
                    暂无项目
                  </div>
                ) : (
                  categoryProjects.map(project => {
                    const isExpanded = expanded.has(project.id)
                    const currentBusy = busy[project.id]
                    const isDesktop = project.kind === 'desktop'
                    const isActive = project.status === 'running' || project.status === 'partial'
                    return (
                      <div
                        id={project.id}
                        key={project.id}
                        className="rundock-project-row"
                        data-selected={selectedProject?.id === project.id}
                        onClick={() => setSelectedProjectId(project.id)}
                      >
                        <div
                          style={{
                            display: 'flex',
                            alignItems: 'center',
                            gap: 14,
                            padding: '9px 13px',
                            flexWrap: 'wrap',
                            minWidth: 0,
                          }}
                        >
                          <div
                            style={{
                              display: 'flex',
                              alignItems: 'center',
                              gap: 16,
                              flex: '1 1 600px',
                              minWidth: 0,
                              flexWrap: 'wrap',
                            }}
                          >
                            <div
                              style={{
                                display: 'flex',
                                minWidth: 260,
                                alignItems: 'center',
                                gap: 8,
                                flex: '1 1 310px',
                              }}
                            >
                              {isDesktop ? (
                                <span title="桌面软件" aria-hidden="true" style={iconButtonStyle}>
                                  <MonitorUp size={15} />
                                </span>
                              ) : (
                                <button
                                  onClick={() => toggleProject(project.id)}
                                  title={isExpanded ? '收起技术组件' : '展开技术组件'}
                                  style={iconButtonStyle}
                                >
                                  {isExpanded ? (
                                    <ChevronDown size={15} />
                                  ) : (
                                    <ChevronRight size={15} />
                                  )}
                                </button>
                              )}
                              <div
                                data-testid={`project-identity-${project.id}`}
                                style={{
                                  display: 'flex',
                                  alignItems: 'center',
                                  gap: 8,
                                  minWidth: 0,
                                  flex: 1,
                                }}
                              >
                                <span title={project.display_name} style={projectNameStyle}>
                                  {project.display_name}
                                </span>
                                <span aria-hidden="true" style={identityDividerStyle} />
                                {editingNote === project.id ? (
                                  <div
                                    style={{
                                      display: 'flex',
                                      alignItems: 'center',
                                      gap: 5,
                                      minWidth: 0,
                                      flex: '1 1 250px',
                                    }}
                                  >
                                    <input
                                      autoFocus
                                      value={draft}
                                      maxLength={500}
                                      onChange={event => setDraft(event.target.value)}
                                      onKeyDown={event => {
                                        if (event.key === 'Enter') void saveNote(project)
                                        if (event.key === 'Escape') setEditingNote(null)
                                      }}
                                      aria-label={`编辑 ${project.display_name} 备注`}
                                      placeholder="添加备注…"
                                      style={{
                                        ...inputStyle,
                                        width: 210,
                                        maxWidth: '34vw',
                                        height: 28,
                                        fontSize: 11,
                                      }}
                                    />
                                    <button
                                      type="button"
                                      disabled={!!currentBusy}
                                      onClick={() => void saveNote(project)}
                                      aria-label={`保存 ${project.display_name} 备注`}
                                      style={smallButtonStyle}
                                    >
                                      {currentBusy === 'save-note' ? '保存中…' : '保存'}
                                    </button>
                                    <button
                                      type="button"
                                      disabled={!!currentBusy}
                                      onClick={() => setEditingNote(null)}
                                      aria-label={`取消编辑 ${project.display_name} 备注`}
                                      style={smallButtonStyle}
                                    >
                                      取消
                                    </button>
                                  </div>
                                ) : (
                                  <button
                                    type="button"
                                    onClick={() => {
                                      setEditingNote(project.id)
                                      setDraft(project.note)
                                    }}
                                    title={
                                      project.note ? `编辑备注：${project.note}` : '添加项目备注'
                                    }
                                    aria-label={`编辑 ${project.display_name} 备注`}
                                    style={{
                                      ...noteButtonStyle,
                                      color: project.note
                                        ? 'var(--color-muted-foreground)'
                                        : 'var(--color-primary)',
                                    }}
                                  >
                                    <span
                                      style={{
                                        overflow: 'hidden',
                                        textOverflow: 'ellipsis',
                                        whiteSpace: 'nowrap',
                                      }}
                                    >
                                      {project.note || '添加备注'}
                                    </span>
                                    <Pencil size={11} style={{ opacity: 0.7, flexShrink: 0 }} />
                                  </button>
                                )}
                              </div>
                            </div>

                            <div style={summaryStripStyle}>
                              <span
                                style={{
                                  ...statusPillStyle,
                                  color: projectStatusColor(project.status),
                                  borderColor: `color-mix(in srgb, ${projectStatusColor(project.status)} 42%, var(--color-border))`,
                                  background: `color-mix(in srgb, ${projectStatusColor(project.status)} 10%, transparent)`,
                                }}
                              >
                                <span aria-hidden="true">●</span>
                                <span>{projectStatusLabel(project.status)}</span>
                              </span>
                              {!isDesktop && (
                                <>
                                  <span style={metricStyle}>
                                    <strong>
                                      {project.active_process_count}/{project.process_count}
                                    </strong>
                                    &nbsp;组件
                                  </span>
                                  <span aria-hidden="true" style={metricDividerStyle} />
                                  <span style={metricStyle}>
                                    CPU&nbsp;<strong>{project.cpu_percent.toFixed(1)}%</strong>
                                  </span>
                                  <span aria-hidden="true" style={metricDividerStyle} />
                                  <span style={metricStyle}>
                                    内存&nbsp;<strong>{formatBytes(project.memory_bytes)}</strong>
                                  </span>
                                </>
                              )}
                            </div>
                          </div>

                          <div
                            style={{
                              display: 'flex',
                              justifyContent: 'flex-end',
                              gap: 5,
                              alignItems: 'center',
                              flexWrap: 'wrap',
                              marginLeft: 'auto',
                            }}
                          >
                            <select
                              value={project.category}
                              disabled={!!currentBusy}
                              onChange={event => void moveProject(project, event.target.value)}
                              aria-label={`移动 ${project.display_name} 分类`}
                              style={{ ...inputStyle, width: 68, height: 28, padding: '4px' }}
                            >
                              {PROJECT_CATEGORIES.map(item => (
                                <option key={item}>{item}</option>
                              ))}
                            </select>
                            {isDesktop ? (
                              <DesktopLaunchButton launchUri={project.launch_uri} />
                            ) : (
                              <WebPortButton
                                ports={projectPorts.get(project.id) ?? []}
                                preferredPort={project.web_port}
                                server={activeServer.server}
                                showLabel
                              />
                            )}
                            {!isDesktop &&
                              (!project.enabled ? (
                                <ActionButton
                                  icon={Power}
                                  label={currentBusy === 'enable' ? '启用中…' : '启用'}
                                  disabled={!!currentBusy}
                                  primary
                                  onClick={() => void setEnabled(project, true)}
                                />
                              ) : (
                                <>
                                  {isActive ? (
                                    <ActionButton
                                      icon={Square}
                                      label={currentBusy === 'stop' ? '停止中…' : '停止'}
                                      disabled={!!currentBusy}
                                      danger
                                      primary
                                      onClick={() => void runAction(project, 'stop')}
                                    />
                                  ) : (
                                    <ActionButton
                                      icon={Play}
                                      label={currentBusy === 'start' ? '启动中…' : '启动'}
                                      disabled={!!currentBusy}
                                      primary
                                      onClick={() => void runAction(project, 'start')}
                                    />
                                  )}
                                  <ActionButton
                                    icon={RotateCcw}
                                    label={currentBusy === 'restart' ? '重启中…' : '重启'}
                                    disabled={!!currentBusy || !isActive}
                                    onClick={() => void runAction(project, 'restart')}
                                  />
                                  <ActionButton
                                    icon={Power}
                                    label={currentBusy === 'disable' ? '停用中…' : '停用'}
                                    disabled={!!currentBusy}
                                    danger
                                    onClick={() => void setEnabled(project, false)}
                                  />
                                </>
                              ))}
                          </div>
                        </div>

                        {!isDesktop && isExpanded && (
                          <div
                            style={{
                              padding: '0 14px 12px 48px',
                              background: 'var(--color-secondary)',
                            }}
                          >
                            <div
                              style={{
                                padding: '9px 0 6px',
                                fontSize: 10,
                                fontWeight: 700,
                                color: 'var(--color-muted-foreground)',
                                letterSpacing: '0.08em',
                              }}
                            >
                              技术组件（排错时查看）
                            </div>
                            {project.members.map(member => (
                              <div
                                data-testid={`technical-member-${member.id}`}
                                key={member.id}
                                className="rundock-technical-member"
                              >
                                <span style={{ fontWeight: 550 }}>{member.name}</span>
                                <span>
                                  <span style={{ color: statusColor(member.status) }}>●</span>{' '}
                                  {processStatusLabel(member.status)}
                                </span>
                                <span style={{ color: 'var(--color-muted-foreground)' }}>
                                  {member.pid ? `PID ${member.pid}` : '无 PID'}
                                </span>
                                <div
                                  style={{
                                    display: 'flex',
                                    alignItems: 'center',
                                    justifyContent: 'flex-end',
                                    gap: 6,
                                    flexWrap: 'wrap',
                                    minWidth: 0,
                                  }}
                                >
                                  {(member.pid == null
                                    ? []
                                    : (portsByPid.get(member.pid) ?? [])
                                  ).map(port => (
                                    <span
                                      key={port}
                                      style={{
                                        color: 'var(--color-muted-foreground)',
                                        fontSize: 10,
                                      }}
                                    >
                                      :{port}
                                    </span>
                                  ))}
                                  <Link
                                    to={`/processes/${member.id}`}
                                    style={{
                                      color: 'var(--color-primary)',
                                      textDecoration: 'none',
                                      whiteSpace: 'nowrap',
                                    }}
                                  >
                                    日志与端口 →
                                  </Link>
                                </div>
                              </div>
                            ))}
                          </div>
                        )}
                      </div>
                    )
                  })
                )}
              </div>
            </section>
          ))}
        </main>
        <aside className="rundock-inspector" aria-label="项目详情">
          {selectedProject ? (
            <div className="rundock-inspector-card">
              <div style={{ display: 'flex', alignItems: 'center', gap: 11 }}>
                <div className="rundock-project-avatar" aria-hidden="true">
                  {selectedProject.display_name.slice(0, 2).toUpperCase()}
                </div>
                <div style={{ minWidth: 0 }}>
                  <h3
                    style={{
                      margin: 0,
                      overflow: 'hidden',
                      textOverflow: 'ellipsis',
                      whiteSpace: 'nowrap',
                      fontSize: 16,
                    }}
                  >
                    {selectedProject.display_name}
                  </h3>
                  <div
                    style={{
                      display: 'flex',
                      alignItems: 'center',
                      gap: 6,
                      marginTop: 4,
                      color: projectStatusColor(selectedProject.status),
                      fontSize: 11,
                      fontWeight: 650,
                    }}
                  >
                    <span aria-hidden="true">●</span>
                    <span>{projectStatusLabel(selectedProject.status)}</span>
                  </div>
                </div>
              </div>

              <div className="rundock-inspector-actions">
                {selectedProject.kind === 'desktop' ? (
                  <DesktopLaunchButton
                    launchUri={selectedProject.launch_uri}
                    ariaLabelPrefix="项目详情"
                  />
                ) : (
                  <>
                    <WebPortButton
                      ports={projectPorts.get(selectedProject.id) ?? []}
                      preferredPort={selectedProject.web_port}
                      server={activeServer.server}
                      showLabel
                      ariaLabelPrefix="项目详情"
                    />
                    {selectedProject.status === 'running' ||
                    selectedProject.status === 'partial' ? (
                      <ActionButton
                        icon={Square}
                        label="停止"
                        disabled={!!busy[selectedProject.id]}
                        danger
                        primary
                        onClick={() => void runAction(selectedProject, 'stop')}
                      />
                    ) : (
                      <ActionButton
                        icon={Play}
                        label="启动"
                        disabled={!!busy[selectedProject.id] || !selectedProject.enabled}
                        primary
                        onClick={() => void runAction(selectedProject, 'start')}
                      />
                    )}
                    <ActionButton
                      icon={RotateCcw}
                      label="重启"
                      disabled={
                        !!busy[selectedProject.id] || selectedProject.active_process_count === 0
                      }
                      onClick={() => void runAction(selectedProject, 'restart')}
                    />
                  </>
                )}
              </div>

              {selectedProject.note && (
                <p
                  style={{
                    margin: '12px 0 0',
                    color: 'var(--color-muted-foreground)',
                    fontSize: 11,
                    lineHeight: 1.55,
                  }}
                >
                  {selectedProject.note}
                </p>
              )}

              {selectedProject.kind !== 'desktop' && (
                <>
                  <div className="rundock-inspector-section">
                    <div
                      style={{
                        display: 'flex',
                        justifyContent: 'space-between',
                        gap: 8,
                        color: 'var(--color-muted-foreground)',
                        fontSize: 11,
                      }}
                    >
                      <span>组件</span>
                      <strong style={{ color: 'var(--color-foreground)' }}>
                        {selectedProject.active_process_count}/{selectedProject.process_count}
                      </strong>
                    </div>
                    <div className="rundock-inspector-list">
                      {selectedProject.members.map(member => (
                        <Link
                          key={member.id}
                          to={`/processes/${member.id}`}
                          className="rundock-inspector-item"
                          style={{ color: 'inherit', textDecoration: 'none' }}
                        >
                          <span
                            style={{
                              overflow: 'hidden',
                              textOverflow: 'ellipsis',
                              whiteSpace: 'nowrap',
                            }}
                          >
                            {member.name}
                          </span>
                          <span style={{ color: statusColor(member.status), whiteSpace: 'nowrap' }}>
                            ● {processStatusLabel(member.status)}
                          </span>
                        </Link>
                      ))}
                    </div>
                  </div>

                  <div className="rundock-inspector-section">
                    <div style={{ color: 'var(--color-muted-foreground)', fontSize: 11 }}>
                      开放端口
                    </div>
                    <div className="rundock-inspector-list">
                      {(projectPorts.get(selectedProject.id) ?? []).length === 0 ? (
                        <div className="rundock-inspector-item">
                          <span>暂未检测到监听端口</span>
                        </div>
                      ) : (
                        (projectPorts.get(selectedProject.id) ?? []).map(port => (
                          <div className="rundock-inspector-item" key={port}>
                            <span>端口</span>
                            <strong>{port}</strong>
                          </div>
                        ))
                      )}
                    </div>
                  </div>
                </>
              )}
            </div>
          ) : (
            <div
              className="rundock-inspector-card"
              style={{ color: 'var(--color-muted-foreground)', fontSize: 12 }}
            >
              选择一个项目查看组件、端口和快捷操作。
            </div>
          )}
        </aside>
      </div>
    </div>
  )
}

function ActionButton({
  icon: Icon,
  label,
  disabled,
  danger,
  primary,
  onClick,
}: {
  icon: typeof Play
  label: string
  disabled: boolean
  danger?: boolean
  primary?: boolean
  onClick: () => void
}) {
  const tone = danger ? 'var(--color-destructive)' : 'var(--color-status-running)'
  return (
    <button
      type="button"
      disabled={disabled}
      onClick={onClick}
      style={{
        ...smallButtonStyle,
        height: 28,
        display: 'inline-flex',
        alignItems: 'center',
        gap: 4,
        color: primary ? tone : danger ? 'var(--color-destructive)' : 'var(--color-foreground)',
        background: primary
          ? `color-mix(in srgb, ${tone} 11%, var(--color-secondary))`
          : 'var(--color-secondary)',
        borderColor: primary
          ? `color-mix(in srgb, ${tone} 48%, var(--color-border))`
          : 'var(--color-border)',
        opacity: disabled ? 0.45 : 1,
        cursor: disabled ? 'not-allowed' : 'pointer',
      }}
    >
      <Icon size={11} /> {label}
    </button>
  )
}

const inputStyle: React.CSSProperties = {
  minWidth: 0,
  padding: '6px 8px',
  borderRadius: 5,
  border: '1px solid var(--color-border)',
  background: 'var(--color-background)',
  color: 'var(--color-foreground)',
  outline: 'none',
  boxSizing: 'border-box',
}

const countPillStyle: React.CSSProperties = {
  fontSize: 10,
  color: 'var(--color-muted-foreground)',
  background: 'var(--color-muted)',
  padding: '1px 7px',
  borderRadius: 8,
}

const feedbackStyle: React.CSSProperties = {
  fontSize: 12,
  padding: '7px 9px',
  borderRadius: 5,
  background: 'var(--color-secondary)',
  border: '1px solid var(--color-border)',
}

const iconButtonStyle: React.CSSProperties = {
  width: 24,
  height: 24,
  padding: 0,
  border: 'none',
  background: 'transparent',
  color: 'var(--color-muted-foreground)',
  cursor: 'pointer',
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'center',
}

const projectNameStyle: React.CSSProperties = {
  minWidth: 0,
  maxWidth: 220,
  overflow: 'hidden',
  textOverflow: 'ellipsis',
  whiteSpace: 'nowrap',
  color: 'var(--color-foreground)',
  fontSize: 14,
  fontWeight: 650,
  flexShrink: 0,
}

const identityDividerStyle: React.CSSProperties = {
  width: 1,
  height: 14,
  background: 'var(--color-border)',
  flexShrink: 0,
}

const noteButtonStyle: React.CSSProperties = {
  display: 'inline-flex',
  alignItems: 'center',
  gap: 5,
  minWidth: 0,
  maxWidth: 300,
  width: 'auto',
  padding: '3px 6px',
  border: '1px solid transparent',
  borderRadius: 5,
  background: 'var(--color-secondary)',
  fontSize: 11,
  textAlign: 'left',
  cursor: 'pointer',
  overflow: 'hidden',
  flex: '0 1 auto',
}

const summaryStripStyle: React.CSSProperties = {
  display: 'flex',
  alignItems: 'center',
  gap: 8,
  minWidth: 0,
  flexWrap: 'wrap',
  color: 'var(--color-muted-foreground)',
  fontSize: 11,
}

const statusPillStyle: React.CSSProperties = {
  display: 'inline-flex',
  alignItems: 'center',
  gap: 5,
  height: 24,
  padding: '0 8px',
  border: '1px solid var(--color-border)',
  borderRadius: 999,
  fontSize: 11,
  fontWeight: 600,
  whiteSpace: 'nowrap',
}

const metricStyle: React.CSSProperties = {
  display: 'inline-flex',
  alignItems: 'center',
  whiteSpace: 'nowrap',
}

const metricDividerStyle: React.CSSProperties = {
  width: 1,
  height: 12,
  background: 'var(--color-border)',
}

const smallButtonStyle: React.CSSProperties = {
  padding: '5px 7px',
  borderRadius: 5,
  border: '1px solid var(--color-border)',
  background: 'var(--color-secondary)',
  color: 'var(--color-foreground)',
  fontSize: 11,
  whiteSpace: 'nowrap',
}
