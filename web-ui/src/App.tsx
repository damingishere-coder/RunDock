// @group BusinessLogic : Root app — layout shell + React Router

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { ReactQueryDevtoolsPanel } from '@tanstack/react-query-devtools'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import LoginPage from '@/pages/LoginPage'
import { clearSessionToken, isAuthenticated } from '@/lib/auth'
import {
  BrowserRouter,
  Link,
  Navigate,
  Route,
  Routes,
  useLocation,
  useNavigate,
} from 'react-router-dom'
import {
  LayoutGrid,
  Plus,
  Clock,
  ScrollText,
  Settings,
  Bell,
  Bot,
  Network,
  BarChart2,
  Server,
  Save,
  Lock,
  Power,
  Globe,
  RotateCcw,
  Square,
  Menu,
  Search,
  type LucideIcon,
} from 'lucide-react'
import {
  getActiveServer,
  getActiveServerId,
  getServers,
  LOCAL_SERVER,
  saveServers,
  setActiveServerId,
  sshTunnelCommand,
  type RemoteServer,
} from '@/lib/servers'
import { useDaemonHealth } from '@/hooks/useDaemonHealth'
import { useProcesses } from '@/hooks/useProcesses'
import { useProjects } from '@/hooks/useProjects'
import { useSettings } from '@/hooks/useSettings'
import { useDialog } from '@/hooks/useDialog'
import { useNotificationTray } from '@/hooks/useNotificationTray'
import { Dialog } from '@/components/Dialog'
import { DiscordIcon } from '@/components/DiscordIcon'
import { GitHubStarBanner, GitHubStarWidget } from '@/components/GitHubStarBanner'
import { NotificationTray } from '@/components/NotificationTray'
import { AiPanel } from '@/components/AiPanel'
import {
  TerminalPanel,
  TerminalStatusBarBtn,
  type TerminalPanelHandle,
  type TerminalPanelState,
  type TerminalShortcuts,
} from '@/components/TerminalPanel'
import { api } from '@/lib/api'
import ProjectsPage from '@/pages/ProjectsPage'
import CronJobsPage from '@/pages/CronJobsPage'
import CreateCronJobPage from '@/pages/CreateCronJobPage'
import StartPage from '@/pages/StartPage'
import EditPage from '@/pages/EditPage'
import ProcessDetailPage from '@/pages/ProcessDetailPage'
import SettingsPage from '@/pages/SettingsPage'
import AnalyticsPage from '@/pages/AnalyticsPage'
import LogLibraryPage from '@/pages/LogLibraryPage'
import LogVolumePage from '@/pages/LogVolumePage'
import NotificationsPage from '@/pages/NotificationsPage'
import PortFinderPage from '@/pages/PortFinderPage'
import TunnelsPage from '@/pages/TunnelsPage'
import type { ProcessInfo, ProjectInfo, UpdateInfo } from '@/types'
import { projectStatusColor, projectStatusLabel } from '@/lib/projects'

// @group BusinessLogic > Layout : Sidebar + content shell
function Layout({ onLock, canLock }: { onLock: () => void; canLock: boolean }) {
  const { settings, updateSettings, resetToDefaults } = useSettings()
  const {
    processes,
    error,
    reload: reloadProcesses,
  } = useProcesses(settings.autoRefresh, settings.processRefreshInterval)
  const {
    projects,
    error: projectsError,
    reload: reloadProjects,
  } = useProjects(settings.autoRefresh, settings.processRefreshInterval)
  const reload = useCallback(() => {
    void reloadProcesses()
    void reloadProjects()
  }, [reloadProcesses, reloadProjects])
  const health = useDaemonHealth(settings.healthRefreshInterval)
  const navigate = useNavigate()
  const location = useLocation()
  const { dialogState, confirm, alert, handleConfirm, handleCancel } = useDialog()

  // @group BusinessLogic > NotificationTray : In-app activity tray
  const { notifications, unreadCount, markAllRead, clearAll, dismiss } =
    useNotificationTray(processes)
  const [trayOpen, setTrayOpen] = useState(false)

  const closeTray = () => setTrayOpen(false)
  const toggleTray = () => {
    if (trayOpen) setTrayOpen(false)
    else {
      setAiOpen(false)
      setTrayOpen(true)
      markAllRead()
    }
  }

  // @group BusinessLogic > AiPanel : AI assistant panel state
  const [aiOpen, setAiOpen] = useState(false)
  const [aiProcessId, setAiProcessId] = useState<string | null>(null)
  const [aiProcessName, setAiProcessName] = useState<string | null>(null)

  const openAi = (processId?: string, processName?: string) => {
    setTrayOpen(false)
    setAiProcessId(processId ?? null)
    setAiProcessName(processName ?? null)
    setAiOpen(true)
  }
  const closeAi = () => setAiOpen(false)

  const connected = error === null

  // @group BusinessLogic > Update : Check for new version once on initial load
  const [updateInfo, setUpdateInfo] = useState<UpdateInfo | null>(null)
  useEffect(() => {
    api
      .checkUpdate()
      .then(info => {
        if (!info.up_to_date) setUpdateInfo(info)
      })
      .catch(() => {})
  }, [])

  const [statsOpen, setStatsOpen] = useState(false)
  const [devtoolsOpen, setDevtoolsOpen] = useState(false)

  // @group BusinessLogic > Terminal : Panel state and tab count for the status bar badge
  const [terminalState, setTerminalState] = useState<TerminalPanelState>('hidden')
  const [terminalTabCount, setTerminalTabCount] = useState(0)
  const terminalPanelRef = useRef<TerminalPanelHandle>(null)

  function toggleTerminal() {
    setTerminalState(s => (s === 'hidden' ? 'normal' : 'hidden'))
  }

  function openTerminalAtCwd(cwd: string, name?: string) {
    setTerminalState(s => (s === 'hidden' ? 'normal' : s))
    // Small delay lets the panel mount/show before opening the tab
    setTimeout(() => terminalPanelRef.current?.openTab(cwd, name), 50)
  }

  // @group BusinessLogic > SidebarList : One row per active logical project
  const [sidebarSearch, setSidebarSearch] = useState('')
  const [collapsedCategories, setCollapsedCategories] = useState<Set<string>>(new Set())

  function toggleSidebarCategory(category: string) {
    setCollapsedCategories(prev => {
      const next = new Set(prev)
      if (next.has(category)) next.delete(category)
      else next.add(category)
      return next
    })
  }

  const sidebarProjectGroups = useMemo(() => {
    let active = projects.filter(
      project =>
        project.status === 'desktop' || project.status === 'running' || project.status === 'partial'
    )
    if (sidebarSearch.trim()) {
      const q = sidebarSearch.toLowerCase()
      active = active.filter(
        project =>
          project.display_name.toLowerCase().includes(q) ||
          project.note.toLowerCase().includes(q) ||
          project.members.some(member => member.name.toLowerCase().includes(q))
      )
    }
    active.sort((a, b) => a.display_name.localeCompare(b.display_name, 'zh-CN'))
    const map = new Map<string, ProjectInfo[]>()
    for (const project of active) {
      const category = project.category === '待定' ? '待定' : '常用'
      if (!map.has(category)) map.set(category, [])
      map.get(category)!.push(project)
    }
    return [...map.entries()].sort(([a], [b]) =>
      a === '常用' ? -1 : b === '常用' ? 1 : a.localeCompare(b, 'zh-CN')
    )
  }, [projects, sidebarSearch])

  const totalActive = useMemo(
    () => sidebarProjectGroups.reduce((sum, [, items]) => sum + items.length, 0),
    [sidebarProjectGroups]
  )

  async function handleSave() {
    await api.saveState().catch(() => {})
    await alert('状态已保存', '进程状态已保存到磁盘。')
  }

  async function handleShutdown() {
    if (settings.confirmBeforeShutdown) {
      const ok = await confirm('关闭守护进程？', 'RunDock 守护进程将停止，受管理的进程会继续运行。')
      if (!ok) return
    }
    await api.shutdownDaemon().catch(() => {})
  }

  const isProcessActive =
    location.pathname === '/processes' || location.pathname.startsWith('/processes/')
  const isCronActive = location.pathname === '/cron-jobs' || location.pathname === '/cron-jobs/new'
  const isPortsActive = location.pathname === '/ports'
  const isTunnelsActive = location.pathname === '/tunnels'

  const [cronOpen, setCronOpen] = useState(false)
  const cronJobs = useMemo(() => processes.filter(p => p.cron), [processes])
  const [toolsOpen, setToolsOpen] = useState(false)
  const [sidebarOpen, setSidebarOpen] = useState(false)
  const [globalSearch, setGlobalSearch] = useState('')

  const pageTitle = useMemo(() => {
    const path = location.pathname
    if (path === '/') return '运行概览'
    if (path.startsWith('/processes')) return '项目总览'
    if (path.startsWith('/cron-jobs')) return '计划任务'
    if (path.startsWith('/logs')) return '日志库'
    if (path.startsWith('/log-volume')) return '日志分析'
    if (path.startsWith('/notifications')) return '通知中心'
    if (path.startsWith('/ports')) return '端口查找'
    if (path.startsWith('/tunnels')) return '隧道'
    if (path.startsWith('/settings')) return '设置'
    if (path.startsWith('/start')) return '添加项目组件'
    return 'RunDock'
  }, [location.pathname])

  return (
    <div className="rundock-shell">
      {/* Global dialog — rendered at root so it overlays everything */}
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

      {/* Activity tray — slides in over main content */}
      <NotificationTray
        open={trayOpen}
        notifications={notifications}
        onClose={closeTray}
        onMarkAllRead={markAllRead}
        onClearAll={clearAll}
        onDismiss={dismiss}
      />

      {/* AI assistant panel — slides in from right */}
      <AiPanel
        open={aiOpen}
        processId={aiProcessId}
        processName={aiProcessName}
        onClose={closeAi}
      />

      {/* Main row: sidebar + content */}
      <div className="rundock-main-row">
        {sidebarOpen && (
          <button
            type="button"
            className="rundock-sidebar-backdrop"
            aria-label="关闭导航"
            onClick={() => setSidebarOpen(false)}
          />
        )}

        {/* Sidebar */}
        <aside
          className="rundock-sidebar"
          data-open={sidebarOpen}
          onClickCapture={event => {
            if ((event.target as HTMLElement).closest('a')) setSidebarOpen(false)
          }}
        >
          {/* Logo */}
          <div className="rundock-logo-wrap">
            <Link to="/" className="rundock-logo-link" aria-label="RunDock 运行概览">
              <img className="rundock-logo-icon" src="/rundock-icon.svg" alt="" />
              <span className="rundock-logo-text">
                Run<strong>Dock</strong>
              </span>
            </Link>
          </div>

          {/* Nav */}
          <nav className="rundock-nav" aria-label="主导航">
            {/* Project row with inline + button */}
            <NavRowWithAdd
              to="/processes"
              icon={LayoutGrid}
              label="项目"
              active={isProcessActive}
              onAdd={() => {
                setSidebarOpen(false)
                navigate('/start')
              }}
              addTitle="添加新项目组件"
            />

            {/* Cron Jobs row with inline + button */}
            <NavRowWithAdd
              to="/cron-jobs"
              icon={Clock}
              label="定时任务"
              active={isCronActive}
              onAdd={() => {
                setSidebarOpen(false)
                navigate('/cron-jobs/new')
              }}
              addTitle="新建定时任务"
              onToggleNs={cronJobs.length > 0 ? () => setCronOpen(v => !v) : undefined}
              nsOpen={cronOpen}
            />

            {/* Cron job submenu — collapsible namespace list for cron jobs */}
            <CronJobSubmenu processes={processes} currentNamespace={null} open={cronOpen} />

            <div style={{ height: 4 }} />
            <NavBtn
              to="/logs"
              icon={ScrollText}
              label="日志库"
              active={location.pathname === '/logs'}
            />
            <NavBtn
              to="/log-volume"
              icon={BarChart2}
              label="日志量"
              active={location.pathname === '/log-volume'}
            />

            {/* Tools section — collapsible */}
            <button
              onClick={() => setToolsOpen(v => !v)}
              style={{
                display: 'flex',
                alignItems: 'center',
                width: '100%',
                padding: '8px 16px 4px',
                background: 'transparent',
                border: 'none',
                cursor: 'pointer',
                textAlign: 'left',
                fontFamily: 'inherit',
              }}
            >
              <span
                style={{
                  fontSize: 9,
                  fontWeight: 700,
                  letterSpacing: '0.1em',
                  color: 'var(--color-muted-foreground)',
                  textTransform: 'uppercase',
                  opacity: 0.6,
                  flex: 1,
                }}
              >
                工具
              </span>
              <span
                style={{
                  fontSize: 8,
                  color: 'var(--color-muted-foreground)',
                  opacity: 0.5,
                  display: 'inline-block',
                  transform: toolsOpen ? 'rotate(0deg)' : 'rotate(-90deg)',
                  transition: 'transform 0.15s',
                }}
              >
                ▼
              </span>
            </button>
            {toolsOpen && (
              <>
                <NavBtn to="/ports" icon={Network} label="端口查找" active={isPortsActive} />
                <NavBtn to="/tunnels" icon={Globe} label="隧道" active={isTunnelsActive} />
              </>
            )}
          </nav>

          {/* Active projects list */}
          <div className="rundock-sidebar-content">
            <div style={{ padding: '6px 8px 4px', flexShrink: 0 }}>
              <input
                value={sidebarSearch}
                onChange={e => setSidebarSearch(e.target.value)}
                placeholder="筛选项目…"
                className="rundock-sidebar-search"
              />
            </div>
            <div style={{ padding: '2px 0', flex: 1, overflow: 'auto' }}>
              <div
                style={{
                  fontSize: 10,
                  fontWeight: 600,
                  color: 'var(--color-muted-foreground)',
                  padding: '4px 16px 6px',
                  letterSpacing: '0.08em',
                }}
              >
                活动项目{' '}
                {totalActive > 0 && (
                  <span style={{ fontWeight: 400, opacity: 0.7 }}>({totalActive})</span>
                )}
              </div>
              {sidebarProjectGroups.length === 0 ? (
                <div
                  style={{
                    fontSize: 12,
                    color: 'var(--color-muted-foreground)',
                    padding: '4px 16px',
                  }}
                >
                  {sidebarSearch ? '没有匹配项' : '没有活动项目'}
                </div>
              ) : (
                sidebarProjectGroups.map(([category, items]) => (
                  <SidebarProjectGroup
                    key={category}
                    category={category}
                    projects={items}
                    collapsed={collapsedCategories.has(category)}
                    onToggle={() => toggleSidebarCategory(category)}
                    onNavigate={project => {
                      setSidebarOpen(false)
                      navigate(`/processes#${project.id}`)
                    }}
                    onStop={async project => {
                      const response = await api.stopProject(project.id)
                      const failure = response.results.find(result => !result.success)
                      if (failure)
                        throw new Error(`${failure.name}：${failure.error ?? '停止失败'}`)
                      reload()
                    }}
                    onRestart={async project => {
                      const response = await api.restartProject(project.id)
                      const failure = response.results.find(result => !result.success)
                      if (failure)
                        throw new Error(`${failure.name}：${failure.error ?? '重启失败'}`)
                      reload()
                    }}
                    onError={message => void alert('项目操作失败', message)}
                  />
                ))
              )}
            </div>
          </div>

          {/* Footer */}
          <div className="rundock-sidebar-footer">
            <ServerSwitcher />
            {/* Icon row: Settings + Save + Lock + Shutdown — equally spaced */}
            <div style={{ display: 'flex', gap: 4 }}>
              <IconBtn
                icon={Settings}
                title="设置"
                onClick={() => {
                  setSidebarOpen(false)
                  navigate('/settings')
                }}
                active={location.pathname.startsWith('/settings')}
                badge={updateInfo !== null}
              />
              <div style={{ flex: 1 }} />
              <IconBtn icon={Save} title="保存状态" onClick={handleSave} />
              {canLock && <IconBtn icon={Lock} title="锁定屏幕" onClick={onLock} />}
              <IconBtn icon={Power} title="关闭守护进程" onClick={handleShutdown} danger />
            </div>
          </div>
        </aside>

        {/* Floating system stats widget */}
        {statsOpen && <SystemStatsWidget onClose={() => setStatsOpen(false)} />}

        {/* Main workspace */}
        <section className="rundock-workspace">
          <header className="rundock-topbar">
            <button
              type="button"
              className="rundock-mobile-menu"
              aria-label="打开导航"
              aria-expanded={sidebarOpen}
              onClick={() => setSidebarOpen(value => !value)}
            >
              <Menu size={18} />
            </button>
            <div className="rundock-topbar-title">{pageTitle}</div>
            <form
              className="rundock-global-search"
              role="search"
              onSubmit={event => {
                event.preventDefault()
                const query = globalSearch.trim()
                if (query) navigate(`/processes?q=${encodeURIComponent(query)}`)
              }}
            >
              <Search size={16} aria-hidden="true" />
              <input
                value={globalSearch}
                onChange={event => setGlobalSearch(event.target.value)}
                aria-label="搜索项目或组件"
                placeholder="搜索项目或组件"
              />
            </form>
            <div
              className="rundock-health-pill"
              data-connected={connected}
              title={connected ? '守护进程连接正常' : '守护进程未连接'}
            >
              <span className="rundock-health-dot" />
              <span>{connected ? '连接正常' : '连接断开'}</span>
            </div>
            <div className="rundock-topbar-actions">
              <button
                type="button"
                className="rundock-topbar-button"
                aria-label="通知中心"
                onClick={toggleTray}
              >
                <Bell size={17} />
                {unreadCount > 0 && (
                  <span className="rundock-notification-badge">{Math.min(unreadCount, 99)}</span>
                )}
              </button>
              <button
                type="button"
                className="rundock-topbar-button rundock-settings-button"
                aria-label="设置"
                onClick={() => navigate('/settings')}
              >
                <Settings size={17} />
              </button>
            </div>
          </header>
          <div className="rundock-route-content">
            <Routes>
              <Route
                path="/"
                element={
                  <AnalyticsPage processes={processes} settings={settings} reload={reload} />
                }
              />
              <Route
                path="/processes"
                element={
                  <ProjectsPage
                    key={`${location.search}:${location.hash}`}
                    projects={projects}
                    error={projectsError}
                    reload={reload}
                  />
                }
              />
              <Route path="/namespace/:name" element={<Navigate to="/processes" replace />} />
              <Route
                path="/start"
                element={
                  <StartPage
                    onDone={() => {
                      reload()
                      navigate('/processes')
                    }}
                    settings={settings}
                  />
                }
              />
              <Route
                path="/edit/:id"
                element={
                  <EditPage
                    onDone={() => {
                      reload()
                      navigate('/processes')
                    }}
                  />
                }
              />
              <Route
                path="/processes/:id"
                element={
                  <ProcessDetailPage
                    reload={reload}
                    settings={settings}
                    onOpenTerminal={openTerminalAtCwd}
                  />
                }
              />
              <Route
                path="/cron-jobs"
                element={<CronJobsPage processes={processes} reload={reload} settings={settings} />}
              />
              <Route
                path="/cron-jobs/new"
                element={
                  <CreateCronJobPage
                    onDone={() => {
                      reload()
                      navigate('/cron-jobs')
                    }}
                    settings={settings}
                  />
                }
              />
              <Route
                path="/logs"
                element={<LogLibraryPage processes={processes} reload={reload} />}
              />
              <Route path="/log-volume" element={<LogVolumePage processes={processes} />} />
              <Route path="/notifications" element={<NotificationsPage />} />
              <Route path="/ports" element={<PortFinderPage />} />
              <Route path="/tunnels" element={<TunnelsPage />} />
              <Route
                path="/settings/:tab?"
                element={
                  <SettingsPage
                    settings={settings}
                    onUpdate={updateSettings}
                    onReset={resetToDefaults}
                  />
                }
              />
            </Routes>
          </div>
        </section>
      </div>
      {/* end main row */}

      {/* Browser terminal panel — floats above status bar */}
      <TerminalPanel
        ref={terminalPanelRef}
        panelState={terminalState}
        onChangePanelState={setTerminalState}
        onTabCountChange={setTerminalTabCount}
        shortcuts={settings.terminalShortcuts as TerminalShortcuts}
      />

      {/* VSCode-style status bar */}
      <StatusBar
        connected={connected}
        projects={projects}
        statsOpen={statsOpen}
        onToggleStats={() => setStatsOpen(v => !v)}
        updateInfo={updateInfo}
        onGoToUpdate={() => navigate('/settings')}
        version={health?.version ?? null}
        unreadCount={unreadCount}
        trayOpen={trayOpen}
        onToggleTray={toggleTray}
        aiOpen={aiOpen}
        onToggleAi={() => {
          if (aiOpen) {
            closeAi()
            return
          }
          const match = location.pathname.match(/^\/processes\/([^/]+)$/)
          if (match) {
            const proc = processes.find(p => p.id === match[1] || p.name === match[1])
            openAi(match[1], proc?.name)
          } else {
            openAi()
          }
        }}
        devtoolsEnabled={import.meta.env.DEV && settings.showQueryDevtools}
        devtoolsOpen={devtoolsOpen}
        onToggleDevtools={() => setDevtoolsOpen(v => !v)}
        terminalState={terminalState}
        terminalTabCount={terminalTabCount}
        onToggleTerminal={toggleTerminal}
      />
      {import.meta.env.DEV && settings.showQueryDevtools && devtoolsOpen && (
        <ReactQueryDevtoolsPanel
          onClose={() => setDevtoolsOpen(false)}
          style={{ maxHeight: 400 }}
        />
      )}
    </div>
  )
}

// @group BusinessLogic > CronJobSubmenu : Namespace list for cron jobs — same pattern as NamespaceSubmenu
function CronJobSubmenu({
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
    () => [...new Set(cronJobs.map(p => p.namespace || 'default'))].sort(),
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
            to={`/namespace/${ns}`}
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

// @group BusinessLogic > NavRowWithAdd : Sidebar nav link with optional ▼ submenu toggle and inline + button
function NavRowWithAdd({
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
        {/* ▼ chevron lives inside the link row, intercepts its own click */}
        {onToggleNs && (
          <span
            onClick={e => {
              e.preventDefault()
              e.stopPropagation()
              onToggleNs()
            }}
            title={nsOpen ? '收起列表' : '展开列表'}
            style={{
              marginLeft: 'auto',
              paddingRight: 2,
              display: 'inline-flex',
              alignItems: 'center',
              cursor: 'pointer',
              fontSize: 9,
              opacity: 0.55,
              transform: nsOpen ? 'rotate(0deg)' : 'rotate(-90deg)',
              transition: 'transform 0.15s',
              flexShrink: 0,
            }}
          >
            ▼
          </span>
        )}
      </Link>
      {/* Inline + button */}
      <button
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
function NavBtn({
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

// @group BusinessLogic > SidebarProjectGroup : Category header with project-level rows only
function SidebarProjectGroup({
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
  return (
    <>
      <button
        onClick={onToggle}
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
      {!collapsed &&
        projects.map(project => (
          <SidebarProject
            key={project.id}
            project={project}
            onNavigate={() => onNavigate(project)}
            onStop={() => onStop(project)}
            onRestart={() => onRestart(project)}
            onError={onError}
          />
        ))}
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
        {!hovered && project.process_count > 1 && (
          <span style={{ fontSize: 9, color: 'var(--color-muted-foreground)' }}>
            {project.process_count}
          </span>
        )}
      </button>
      {hovered && project.kind !== 'desktop' && (
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

// @group BusinessLogic > SystemStatsWidget : Bar color helper and StatRow — declared outside component to avoid re-creation on render
function statsBarColor(pct: number) {
  if (pct >= 90) return 'var(--color-status-crashed)'
  if (pct >= 70) return '#f97316'
  return 'var(--color-primary)'
}

function StatRow({ label, pct, detail }: { label: string; pct: number; detail: string }) {
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
      <span
        style={{
          fontSize: 10,
          fontWeight: 700,
          color: 'var(--color-muted-foreground)',
          width: 32,
          flexShrink: 0,
        }}
      >
        {label}
      </span>
      <div
        style={{
          flex: 1,
          height: 5,
          background: 'var(--color-border)',
          borderRadius: 3,
          overflow: 'hidden',
        }}
      >
        <div
          style={{
            height: '100%',
            borderRadius: 3,
            width: `${Math.min(pct, 100).toFixed(1)}%`,
            background: statsBarColor(pct),
            transition: 'width 0.4s ease',
          }}
        />
      </div>
      <span
        style={{
          fontSize: 10,
          color: 'var(--color-muted-foreground)',
          flexShrink: 0,
          minWidth: 64,
          textAlign: 'right',
        }}
      >
        {detail}
      </span>
    </div>
  )
}

// @group BusinessLogic > SystemStatsWidget : Floating draggable CPU / RAM / GPU usage widget
function SystemStatsWidget({ onClose }: { onClose: () => void }) {
  const [stats, setStats] = useState<{
    cpu_percent: number
    ram_used_bytes: number
    ram_total_bytes: number
    gpu: {
      name: string
      utilization_percent: number
      vram_used_bytes: number
      vram_total_bytes: number
    } | null
  } | null>(null)

  // @group BusinessLogic > SystemStatsWidget : Dragging state
  const [pos, setPos] = useState({ x: window.innerWidth - 260, y: 80 })
  const dragging = useRef(false)
  const dragOffset = useRef({ x: 0, y: 0 })

  function onMouseDown(e: React.MouseEvent) {
    dragging.current = true
    dragOffset.current = { x: e.clientX - pos.x, y: e.clientY - pos.y }
    e.preventDefault()
  }

  useEffect(() => {
    function onMove(e: MouseEvent) {
      if (!dragging.current) return
      setPos({
        x: Math.max(0, Math.min(window.innerWidth - 240, e.clientX - dragOffset.current.x)),
        y: Math.max(0, Math.min(window.innerHeight - 120, e.clientY - dragOffset.current.y)),
      })
    }
    function onUp() {
      dragging.current = false
    }
    window.addEventListener('mousemove', onMove)
    window.addEventListener('mouseup', onUp)
    return () => {
      window.removeEventListener('mousemove', onMove)
      window.removeEventListener('mouseup', onUp)
    }
  }, [])

  useEffect(() => {
    let cancelled = false
    async function poll() {
      try {
        const s = await api.getSystemStats()
        if (!cancelled) setStats(s)
      } catch {
        /* daemon not ready yet */
      }
    }
    poll()
    const id = setInterval(poll, 3000)
    return () => {
      cancelled = true
      clearInterval(id)
    }
  }, [])

  const ramPct =
    stats && stats.ram_total_bytes > 0 ? (stats.ram_used_bytes / stats.ram_total_bytes) * 100 : 0

  return (
    <div
      style={{
        position: 'fixed',
        left: pos.x,
        top: pos.y,
        zIndex: 500,
        width: 240,
        background: 'var(--color-card)',
        border: '1px solid var(--color-border)',
        borderRadius: 10,
        boxShadow: '0 4px 24px rgba(0,0,0,0.2)',
        overflow: 'hidden',
        userSelect: 'none',
      }}
    >
      {/* Drag handle / title bar */}
      <div
        onMouseDown={onMouseDown}
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 6,
          padding: '8px 12px',
          background: 'var(--color-secondary)',
          borderBottom: '1px solid var(--color-border)',
          cursor: 'grab',
        }}
      >
        <BarChart2 size={12} style={{ opacity: 0.7 }} />
        <span style={{ flex: 1, fontSize: 11, fontWeight: 600, color: 'var(--color-foreground)' }}>
          系统统计
        </span>
        <button
          onClick={onClose}
          style={{
            width: 18,
            height: 18,
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            background: 'transparent',
            border: 'none',
            cursor: 'pointer',
            color: 'var(--color-muted-foreground)',
            fontSize: 14,
            borderRadius: 3,
            padding: 0,
          }}
          onMouseEnter={e => {
            e.currentTarget.style.color = 'var(--color-destructive)'
          }}
          onMouseLeave={e => {
            e.currentTarget.style.color = 'var(--color-muted-foreground)'
          }}
        >
          ×
        </button>
      </div>

      {/* Stats */}
      <div style={{ padding: '10px 14px', display: 'flex', flexDirection: 'column', gap: 8 }}>
        {!stats ? (
          <div
            style={{
              fontSize: 11,
              color: 'var(--color-muted-foreground)',
              textAlign: 'center',
              padding: '4px 0',
            }}
          >
            连接中…
          </div>
        ) : (
          <>
            <StatRow
              label="CPU"
              pct={stats.cpu_percent}
              detail={`${stats.cpu_percent.toFixed(1)}%`}
            />
            <StatRow
              label="RAM"
              pct={ramPct}
              detail={`${(stats.ram_used_bytes / 1073741824).toFixed(1)} / ${(stats.ram_total_bytes / 1073741824).toFixed(1)} GB`}
            />
            {stats.gpu && (
              <StatRow
                label="GPU"
                pct={stats.gpu.utilization_percent}
                detail={`${stats.gpu.utilization_percent.toFixed(0)}%`}
              />
            )}
            {stats.gpu && (
              <StatRow
                label="VRAM"
                pct={(stats.gpu.vram_used_bytes / stats.gpu.vram_total_bytes) * 100}
                detail={`${(stats.gpu.vram_used_bytes / 1073741824).toFixed(1)} / ${(stats.gpu.vram_total_bytes / 1073741824).toFixed(1)} GB`}
              />
            )}
            {stats.gpu && (
              <div
                style={{
                  fontSize: 9,
                  color: 'var(--color-muted-foreground)',
                  marginTop: -2,
                  opacity: 0.7,
                }}
              >
                {stats.gpu.name}
              </div>
            )}
          </>
        )}
      </div>
    </div>
  )
}

// @group BusinessLogic > ServerSwitcher : Multi-server connection manager in sidebar footer
// @group BusinessLogic > StatusBar > Menu : Overflow dropdown for status bar actions
// @group BusinessLogic > StatusBar : VSCode-style status bar — fixed at viewport bottom
function StatusBar({
  connected,
  projects,
  statsOpen,
  onToggleStats,
  updateInfo,
  onGoToUpdate,
  version,
  unreadCount,
  trayOpen,
  onToggleTray,
  aiOpen,
  onToggleAi,
  devtoolsEnabled,
  devtoolsOpen,
  onToggleDevtools,
  terminalState,
  terminalTabCount,
  onToggleTerminal,
}: {
  connected: boolean
  projects: ProjectInfo[]
  statsOpen: boolean
  onToggleStats: () => void
  updateInfo: UpdateInfo | null
  onGoToUpdate: () => void
  version: string | null
  unreadCount: number
  trayOpen: boolean
  onToggleTray: () => void
  aiOpen: boolean
  onToggleAi: () => void
  devtoolsEnabled: boolean
  devtoolsOpen: boolean
  onToggleDevtools: () => void
  terminalState: TerminalPanelState
  terminalTabCount: number
  onToggleTerminal: () => void
}) {
  const activeServer = getActiveServer()
  const running = projects.filter(
    project => project.status === 'running' || project.status === 'partial'
  ).length
  const total = projects.length

  const bar: React.CSSProperties = {
    height: 22,
    minHeight: 22,
    background: '#0a0a0a',
    color: 'var(--color-muted-foreground)',
    borderTop: '1px solid var(--color-border)',
    display: 'flex',
    alignItems: 'center',
    fontSize: 11,
    fontWeight: 500,
    userSelect: 'none',
    zIndex: 400,
    flexShrink: 0,
  }

  const item: React.CSSProperties = {
    display: 'flex',
    alignItems: 'center',
    gap: 4,
    padding: '0 8px',
    height: '100%',
    cursor: 'default',
    opacity: 0.9,
    whiteSpace: 'nowrap',
  }

  const btnItem: React.CSSProperties = {
    ...item,
    cursor: 'pointer',
    background: 'transparent',
    border: 'none',
    color: 'var(--color-muted-foreground)',
    fontFamily: 'inherit',
    fontSize: 11,
    fontWeight: 500,
  }

  return (
    <div style={bar}>
      {/* Left — connection + server */}
      <div style={{ ...item, paddingLeft: 10, gap: 5 }}>
        <span
          style={{
            width: 6,
            height: 6,
            borderRadius: '50%',
            flexShrink: 0,
            background: connected ? '#4ade80' : '#f87171',
            boxShadow: connected ? '0 0 4px #4ade80' : undefined,
          }}
        />
        <span>{activeServer.id === 'local' ? '本地' : activeServer.name}</span>
      </div>

      <div
        style={{
          ...item,
          borderLeft: '1px solid var(--color-border)',
          opacity: 0.65,
          fontSize: 10,
        }}
      >
        {connected ? '运行中' : '离线'}
      </div>

      {/* Version — always visible; orange + arrow when update is available */}
      <button
        onClick={updateInfo ? onGoToUpdate : undefined}
        title={
          updateInfo
            ? `有可用更新：v${updateInfo.latest} — 点击前往设置`
            : version
              ? `RunDock v${version}（alter CLI）`
              : ''
        }
        style={{
          ...btnItem,
          borderLeft: '1px solid var(--color-border)',
          cursor: updateInfo ? 'pointer' : 'default',
          color: updateInfo ? '#f97316' : 'var(--color-muted-foreground)',
          gap: 3,
          padding: '0 8px',
        }}
        onMouseEnter={e => {
          if (updateInfo)
            e.currentTarget.style.background = 'color-mix(in srgb, #f97316 12%, transparent)'
        }}
        onMouseLeave={e => {
          e.currentTarget.style.background = 'transparent'
        }}
      >
        {updateInfo ? (
          <>
            <span style={{ fontSize: 10 }}>↑</span>
            <span>v{updateInfo.latest} 可用</span>
          </>
        ) : version ? (
          <span style={{ opacity: 0.55 }}>v{version}</span>
        ) : null}
      </button>

      {/* Spacer */}
      <div style={{ flex: 1 }} />

      {/* Right — counts + toggles */}
      {total > 0 && (
        <div style={item} title={`${running} 个活动项目 / 共 ${total} 个项目`}>
          <span style={{ opacity: 0.7 }}>▶</span>
          <span>
            {running}/{total}
          </span>
        </div>
      )}

      {/* GitHub star widget */}
      <GitHubStarWidget />

      {/* Discord community link */}
      <a
        href="https://discord.gg/vxerDZgHJg"
        target="_blank"
        rel="noreferrer"
        title="加入 Discord 社区"
        style={{
          ...item,
          borderLeft: '1px solid var(--color-border)',
          textDecoration: 'none',
          color: '#5865F2',
          gap: 4,
          padding: '0 9px',
          opacity: 0.85,
          cursor: 'pointer',
        }}
        onMouseEnter={e => {
          ;(e.currentTarget as HTMLElement).style.opacity = '1'
        }}
        onMouseLeave={e => {
          ;(e.currentTarget as HTMLElement).style.opacity = '0.85'
        }}
      >
        <DiscordIcon size={12} color="#5865F2" />
      </a>

      {/* Notifications bell */}
      <button
        onClick={onToggleTray}
        title={unreadCount > 0 ? `${unreadCount} 条通知` : '通知'}
        style={{
          ...btnItem,
          padding: '0 9px',
          borderLeft: '1px solid var(--color-border)',
          position: 'relative',
          background: trayOpen
            ? 'color-mix(in srgb, var(--color-foreground) 10%, transparent)'
            : 'transparent',
          color:
            trayOpen || unreadCount > 0
              ? 'var(--color-foreground)'
              : 'var(--color-muted-foreground)',
        }}
        onMouseEnter={e => {
          e.currentTarget.style.background =
            'color-mix(in srgb, var(--color-foreground) 10%, transparent)'
        }}
        onMouseLeave={e => {
          e.currentTarget.style.background = trayOpen
            ? 'color-mix(in srgb, var(--color-foreground) 10%, transparent)'
            : 'transparent'
        }}
      >
        <Bell size={12} />
        {unreadCount > 0 && !trayOpen && (
          <span
            style={{
              position: 'absolute',
              top: 2,
              right: 4,
              minWidth: 13,
              height: 13,
              borderRadius: 7,
              background: 'var(--color-destructive)',
              color: '#fff',
              fontSize: 8,
              fontWeight: 700,
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
              padding: '0 2px',
              lineHeight: 1,
            }}
          >
            {unreadCount > 9 ? '9+' : unreadCount}
          </span>
        )}
      </button>

      {/* Terminal toggle */}
      <TerminalStatusBarBtn
        panelState={terminalState}
        onToggle={onToggleTerminal}
        tabCount={terminalTabCount}
      />

      {/* AI assistant */}
      <button
        onClick={onToggleAi}
        title="AI 助手"
        style={{
          ...btnItem,
          padding: '0 9px',
          borderLeft: '1px solid var(--color-border)',
          background: aiOpen
            ? 'color-mix(in srgb, var(--color-foreground) 10%, transparent)'
            : 'transparent',
          color: aiOpen ? 'var(--color-foreground)' : 'var(--color-muted-foreground)',
        }}
        onMouseEnter={e => {
          e.currentTarget.style.background =
            'color-mix(in srgb, var(--color-foreground) 10%, transparent)'
        }}
        onMouseLeave={e => {
          e.currentTarget.style.background = aiOpen
            ? 'color-mix(in srgb, var(--color-foreground) 10%, transparent)'
            : 'transparent'
        }}
      >
        <Bot size={12} />
      </button>

      {/* Stats toggle */}
      <button
        onClick={onToggleStats}
        title="系统统计"
        style={{
          ...btnItem,
          padding: '0 9px',
          borderLeft: '1px solid var(--color-border)',
          background: statsOpen
            ? 'color-mix(in srgb, var(--color-foreground) 10%, transparent)'
            : 'transparent',
        }}
        onMouseEnter={e => {
          e.currentTarget.style.background =
            'color-mix(in srgb, var(--color-foreground) 10%, transparent)'
        }}
        onMouseLeave={e => {
          e.currentTarget.style.background = statsOpen
            ? 'color-mix(in srgb, var(--color-foreground) 10%, transparent)'
            : 'transparent'
        }}
      >
        <BarChart2 size={12} />
      </button>

      {/* RQ devtools toggle — dev mode only, shown when enabled in Settings → UI */}
      {devtoolsEnabled && (
        <button
          onClick={onToggleDevtools}
          title="React Query 开发工具"
          style={{
            ...btnItem,
            padding: '0 9px',
            borderLeft: '1px solid var(--color-border)',
            background: devtoolsOpen
              ? 'color-mix(in srgb, #e11d48 15%, transparent)'
              : 'transparent',
            color: devtoolsOpen ? '#e11d48' : 'var(--color-muted-foreground)',
            fontFamily: 'monospace',
            fontSize: 10,
            letterSpacing: '-0.5px',
          }}
          onMouseEnter={e => {
            e.currentTarget.style.background = 'color-mix(in srgb, #e11d48 15%, transparent)'
          }}
          onMouseLeave={e => {
            e.currentTarget.style.background = devtoolsOpen
              ? 'color-mix(in srgb, #e11d48 15%, transparent)'
              : 'transparent'
          }}
        >
          RQ
        </button>
      )}
    </div>
  )
}

// @group BusinessLogic > ServerSwitcher : Sidebar panel for switching between local + remote alter daemons
function ServerSwitcher() {
  const [open, setOpen] = useState(false)
  const [remotes, setRemotes] = useState<RemoteServer[]>(() => getServers())
  const [activeId, setActiveId] = useState(() => getActiveServerId())
  const [addMode, setAddMode] = useState(false)
  const [connType, setConnType] = useState<'direct' | 'ssh'>('direct')
  const [form, setForm] = useState({
    name: '',
    host: '',
    port: '2999',
    sshHost: '',
    sshPort: '22',
    sshUser: '',
    sshKeyPath: '',
    remoteDaemonPort: '2999',
    localPort: '3001',
  })
  const [formError, setFormError] = useState<string | null>(null)
  const [copiedCmd, setCopiedCmd] = useState(false)
  const activeServer =
    activeId === 'local' ? LOCAL_SERVER : (remotes.find(s => s.id === activeId) ?? LOCAL_SERVER)

  function switchTo(id: string) {
    setActiveServerId(id)
    window.location.reload()
  }

  function resetForm() {
    setForm({
      name: '',
      host: '',
      port: '2999',
      sshHost: '',
      sshPort: '22',
      sshUser: '',
      sshKeyPath: '',
      remoteDaemonPort: '2999',
      localPort: '3001',
    })
    setConnType('direct')
    setFormError(null)
    setCopiedCmd(false)
  }

  function addServer() {
    setFormError(null)
    if (!form.name.trim()) {
      setFormError('名称不能为空')
      return
    }
    let newServer: RemoteServer
    if (connType === 'direct') {
      if (!form.host.trim()) {
        setFormError('主机不能为空')
        return
      }
      const port = parseInt(form.port, 10)
      if (isNaN(port) || port < 1 || port > 65535) {
        setFormError('端口无效')
        return
      }
      newServer = {
        id: crypto.randomUUID(),
        name: form.name.trim(),
        host: form.host.trim(),
        port,
        connectionType: 'direct',
      }
    } else {
      if (!form.sshHost.trim()) {
        setFormError('SSH 主机不能为空')
        return
      }
      if (!form.sshUser.trim()) {
        setFormError('SSH 用户名不能为空')
        return
      }
      const localPort = parseInt(form.localPort, 10)
      if (isNaN(localPort) || localPort < 1 || localPort > 65535) {
        setFormError('本地端口无效')
        return
      }
      const sshPort = parseInt(form.sshPort, 10) || 22
      const remoteDaemonPort = parseInt(form.remoteDaemonPort, 10) || 2999
      newServer = {
        id: crypto.randomUUID(),
        name: form.name.trim(),
        host: '127.0.0.1',
        port: localPort,
        connectionType: 'ssh',
        sshHost: form.sshHost.trim(),
        sshPort,
        sshUser: form.sshUser.trim(),
        sshKeyPath: form.sshKeyPath.trim() || undefined,
        remoteDaemonPort,
      }
    }
    const updated = [...remotes, newServer]
    saveServers(updated)
    setRemotes(updated)
    resetForm()
    setAddMode(false)
  }

  function removeServer(id: string) {
    const updated = remotes.filter(s => s.id !== id)
    saveServers(updated)
    setRemotes(updated)
    if (activeId === id) {
      setActiveServerId('local')
      setActiveId('local')
      window.location.reload()
    }
  }

  function copyTunnelCmd() {
    const preview: RemoteServer = {
      id: 'preview',
      name: '',
      host: '127.0.0.1',
      port: parseInt(form.localPort, 10) || 3001,
      connectionType: 'ssh',
      sshHost: form.sshHost.trim(),
      sshPort: parseInt(form.sshPort, 10) || 22,
      sshUser: form.sshUser.trim(),
      sshKeyPath: form.sshKeyPath.trim() || undefined,
      remoteDaemonPort: parseInt(form.remoteDaemonPort, 10) || 2999,
    }
    navigator.clipboard.writeText(sshTunnelCommand(preview)).then(() => {
      setCopiedCmd(true)
      setTimeout(() => setCopiedCmd(false), 2000)
    })
  }

  const allServers = [LOCAL_SERVER, ...remotes]
  const tabStyle = (active: boolean): React.CSSProperties => ({
    flex: 1,
    padding: '4px 0',
    fontSize: 10,
    fontWeight: active ? 600 : 400,
    background: active ? 'var(--color-primary)' : 'var(--color-secondary)',
    color: active ? 'var(--color-primary-foreground)' : 'var(--color-muted-foreground)',
    border: '1px solid var(--color-border)',
    cursor: 'pointer',
    fontFamily: 'inherit',
    borderRadius: active
      ? connType === 'direct'
        ? '4px 0 0 4px'
        : '0 4px 4px 0'
      : connType === 'ssh'
        ? '4px 0 0 4px'
        : '0 4px 4px 0',
  })

  return (
    <div style={{ position: 'relative' }}>
      {/* Current server indicator */}
      <button
        onClick={() => setOpen(v => !v)}
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 6,
          width: '100%',
          padding: '6px 12px',
          fontSize: 11,
          background: 'var(--color-secondary)',
          border: '1px solid var(--color-border)',
          borderRadius: 6,
          cursor: 'pointer',
          color: 'var(--color-foreground)',
          fontFamily: 'inherit',
          textAlign: 'left',
        }}
        title="切换服务器"
      >
        <Server size={11} style={{ flexShrink: 0, opacity: 0.7 }} />
        <span
          style={{
            flex: 1,
            overflow: 'hidden',
            textOverflow: 'ellipsis',
            whiteSpace: 'nowrap',
            fontWeight: 500,
          }}
        >
          {activeServer.name}
        </span>
        <span style={{ fontSize: 9, opacity: 0.5, flexShrink: 0 }}>
          {activeServer.id === 'local'
            ? 'local'
            : activeServer.connectionType === 'ssh'
              ? `ssh:${activeServer.sshHost}`
              : `${activeServer.host}:${activeServer.port}`}
        </span>
        <span
          style={{
            fontSize: 8,
            opacity: 0.5,
            flexShrink: 0,
            marginLeft: 2,
            display: 'inline-block',
            transform: open ? 'rotate(0deg)' : 'rotate(-90deg)',
            transition: 'transform 0.15s',
          }}
        >
          ▼
        </span>
      </button>

      {/* Dropdown panel */}
      {open && (
        <div
          style={{
            position: 'absolute',
            bottom: '100%',
            left: 0,
            right: 0,
            background: 'var(--color-card)',
            border: '1px solid var(--color-border)',
            borderRadius: 8,
            boxShadow: '0 -4px 16px rgba(0,0,0,0.15)',
            marginBottom: 4,
            zIndex: 300,
            maxHeight: 420,
            overflow: 'auto',
            padding: '6px 0',
          }}
        >
          <div
            style={{
              padding: '2px 10px 6px',
              fontSize: 9,
              fontWeight: 700,
              color: 'var(--color-muted-foreground)',
              letterSpacing: '0.08em',
              textTransform: 'uppercase',
            }}
          >
            服务器
          </div>

          {allServers.map(s => (
            <div
              key={s.id}
              style={{ display: 'flex', alignItems: 'center', gap: 4, padding: '2px 8px' }}
            >
              <button
                onClick={() => {
                  if (s.id !== activeId) switchTo(s.id)
                }}
                style={{
                  flex: 1,
                  display: 'flex',
                  alignItems: 'center',
                  gap: 7,
                  padding: '5px 8px',
                  fontSize: 12,
                  borderRadius: 5,
                  background: s.id === activeId ? 'var(--color-accent)' : 'transparent',
                  border: 'none',
                  cursor: s.id === activeId ? 'default' : 'pointer',
                  color: s.id === activeId ? 'var(--color-primary)' : 'var(--color-foreground)',
                  fontFamily: 'inherit',
                  textAlign: 'left',
                  fontWeight: s.id === activeId ? 600 : 400,
                }}
              >
                <span
                  style={{
                    width: 7,
                    height: 7,
                    borderRadius: '50%',
                    flexShrink: 0,
                    background:
                      s.id === activeId ? 'var(--color-status-running)' : 'var(--color-border)',
                  }}
                />
                <span
                  style={{
                    flex: 1,
                    overflow: 'hidden',
                    textOverflow: 'ellipsis',
                    whiteSpace: 'nowrap',
                  }}
                >
                  {s.name}
                </span>
                <span style={{ fontSize: 9, opacity: 0.5, flexShrink: 0 }}>
                  {s.id === 'local'
                    ? 'localhost'
                    : s.connectionType === 'ssh'
                      ? `SSH → ${s.sshHost}`
                      : `${s.host}:${s.port}`}
                </span>
              </button>
              {s.id !== 'local' && (
                <button
                  onClick={() => removeServer(s.id)}
                  title="移除服务器"
                  style={{
                    width: 20,
                    height: 20,
                    flexShrink: 0,
                    display: 'flex',
                    alignItems: 'center',
                    justifyContent: 'center',
                    background: 'transparent',
                    border: 'none',
                    cursor: 'pointer',
                    color: 'var(--color-muted-foreground)',
                    fontSize: 13,
                    borderRadius: 3,
                    opacity: 0.6,
                  }}
                  onMouseEnter={e => {
                    e.currentTarget.style.color = 'var(--color-destructive)'
                    e.currentTarget.style.opacity = '1'
                  }}
                  onMouseLeave={e => {
                    e.currentTarget.style.color = 'var(--color-muted-foreground)'
                    e.currentTarget.style.opacity = '0.6'
                  }}
                >
                  ×
                </button>
              )}
            </div>
          ))}

          <div style={{ borderTop: '1px solid var(--color-border)', margin: '6px 8px 0' }} />

          {!addMode ? (
            <button
              onClick={() => setAddMode(true)}
              style={{
                display: 'flex',
                alignItems: 'center',
                gap: 6,
                width: '100%',
                padding: '6px 16px',
                fontSize: 11,
                background: 'transparent',
                border: 'none',
                cursor: 'pointer',
                color: 'var(--color-primary)',
                fontFamily: 'inherit',
              }}
            >
              <Plus size={11} /> 添加远程服务器
            </button>
          ) : (
            <div style={{ padding: '8px 10px', display: 'flex', flexDirection: 'column', gap: 6 }}>
              {/* Connection type tabs */}
              <div style={{ display: 'flex', marginBottom: 2 }}>
                <button
                  style={tabStyle(connType === 'direct')}
                  onClick={() => setConnType('direct')}
                >
                  直连
                </button>
                <button style={tabStyle(connType === 'ssh')} onClick={() => setConnType('ssh')}>
                  SSH 隧道
                </button>
              </div>

              {formError && (
                <div style={{ fontSize: 10, color: 'var(--color-destructive)' }}>{formError}</div>
              )}

              <input
                value={form.name}
                onChange={e => setForm(f => ({ ...f, name: e.target.value }))}
                placeholder="名称（例如：生产环境）"
                style={addInputStyle}
              />

              {connType === 'direct' ? (
                <>
                  <input
                    value={form.host}
                    onChange={e => setForm(f => ({ ...f, host: e.target.value }))}
                    placeholder="主机（例如：192.168.1.100）"
                    style={addInputStyle}
                  />
                  <input
                    value={form.port}
                    onChange={e => setForm(f => ({ ...f, port: e.target.value }))}
                    placeholder="守护进程端口（2999）"
                    style={addInputStyle}
                  />
                </>
              ) : (
                <>
                  <input
                    value={form.sshHost}
                    onChange={e => setForm(f => ({ ...f, sshHost: e.target.value }))}
                    placeholder="SSH 主机（例如：myserver.com）"
                    style={addInputStyle}
                  />
                  <div style={{ display: 'flex', gap: 4 }}>
                    <input
                      value={form.sshUser}
                      onChange={e => setForm(f => ({ ...f, sshUser: e.target.value }))}
                      placeholder="用户名"
                      style={{ ...addInputStyle, flex: 1 }}
                    />
                    <input
                      value={form.sshPort}
                      onChange={e => setForm(f => ({ ...f, sshPort: e.target.value }))}
                      placeholder="SSH 端口"
                      style={{ ...addInputStyle, width: 72 }}
                    />
                  </div>
                  <input
                    value={form.sshKeyPath}
                    onChange={e => setForm(f => ({ ...f, sshKeyPath: e.target.value }))}
                    placeholder="SSH 密钥路径（可选，例如：~/.ssh/id_rsa）"
                    style={addInputStyle}
                  />
                  <div style={{ display: 'flex', gap: 4 }}>
                    <input
                      value={form.remoteDaemonPort}
                      onChange={e => setForm(f => ({ ...f, remoteDaemonPort: e.target.value }))}
                      placeholder="远程端口"
                      style={{ ...addInputStyle, flex: 1 }}
                    />
                    <input
                      value={form.localPort}
                      onChange={e => setForm(f => ({ ...f, localPort: e.target.value }))}
                      placeholder="本地转发端口"
                      style={{ ...addInputStyle, flex: 1 }}
                    />
                  </div>
                  {/* SSH tunnel command preview */}
                  {form.sshHost.trim() && form.sshUser.trim() && (
                    <div
                      style={{
                        background: 'var(--color-secondary)',
                        borderRadius: 4,
                        padding: '5px 7px',
                      }}
                    >
                      <div
                        style={{
                          fontSize: 9,
                          color: 'var(--color-muted-foreground)',
                          marginBottom: 3,
                        }}
                      >
                        请先运行此隧道命令：
                      </div>
                      <code
                        style={{
                          fontSize: 9,
                          color: 'var(--color-foreground)',
                          wordBreak: 'break-all',
                          lineHeight: 1.5,
                        }}
                      >
                        {sshTunnelCommand({
                          id: '',
                          name: '',
                          host: '127.0.0.1',
                          port: parseInt(form.localPort, 10) || 3001,
                          connectionType: 'ssh',
                          sshHost: form.sshHost.trim(),
                          sshPort: parseInt(form.sshPort, 10) || 22,
                          sshUser: form.sshUser.trim(),
                          sshKeyPath: form.sshKeyPath.trim() || undefined,
                          remoteDaemonPort: parseInt(form.remoteDaemonPort, 10) || 2999,
                        })}
                      </code>
                      <button
                        onClick={copyTunnelCmd}
                        style={{
                          marginTop: 4,
                          fontSize: 9,
                          background: 'none',
                          border: 'none',
                          cursor: 'pointer',
                          color: copiedCmd ? 'var(--color-status-running)' : 'var(--color-primary)',
                          fontFamily: 'inherit',
                          padding: 0,
                        }}
                      >
                        {copiedCmd ? '✓ 已复制' : '复制命令'}
                      </button>
                    </div>
                  )}
                </>
              )}

              <div style={{ display: 'flex', gap: 5 }}>
                <button
                  onClick={addServer}
                  style={{
                    ...addBtnStyle,
                    background: 'var(--color-primary)',
                    color: 'var(--color-primary-foreground)',
                  }}
                >
                  添加
                </button>
                <button
                  onClick={() => {
                    setAddMode(false)
                    resetForm()
                  }}
                  style={addBtnStyle}
                >
                  取消
                </button>
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  )
}

const addInputStyle: React.CSSProperties = {
  width: '100%',
  padding: '5px 8px',
  fontSize: 11,
  borderRadius: 4,
  border: '1px solid var(--color-border)',
  background: 'var(--color-background)',
  color: 'var(--color-foreground)',
  outline: 'none',
  boxSizing: 'border-box',
}

const addBtnStyle: React.CSSProperties = {
  flex: 1,
  padding: '4px 8px',
  fontSize: 11,
  fontWeight: 500,
  background: 'var(--color-secondary)',
  border: '1px solid var(--color-border)',
  borderRadius: 4,
  cursor: 'pointer',
  color: 'var(--color-foreground)',
  fontFamily: 'inherit',
}

// @group BusinessLogic > IconBtn : Compact icon-only button for sidebar footer actions
function IconBtn({
  icon: Icon,
  title,
  onClick,
  danger,
  active,
  badge,
}: {
  icon: React.ElementType
  title: string
  onClick: () => void
  danger?: boolean
  active?: boolean
  badge?: boolean
}) {
  const baseColor = danger
    ? 'var(--color-destructive)'
    : active
      ? 'var(--color-primary)'
      : 'var(--color-muted-foreground)'
  const baseBg = active ? 'var(--color-accent)' : 'var(--color-secondary)'
  return (
    <div style={{ position: 'relative', flexShrink: 0 }}>
      <button
        onClick={onClick}
        title={title}
        style={{
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          width: 32,
          height: 28,
          borderRadius: 5,
          cursor: 'pointer',
          background: baseBg,
          border: active ? '1px solid var(--color-primary)' : '1px solid var(--color-border)',
          color: baseColor,
        }}
        onMouseEnter={e => {
          e.currentTarget.style.background = danger
            ? 'color-mix(in srgb, var(--color-destructive) 12%, transparent)'
            : 'var(--color-accent)'
          e.currentTarget.style.color = danger
            ? 'var(--color-destructive)'
            : 'var(--color-foreground)'
        }}
        onMouseLeave={e => {
          e.currentTarget.style.background = baseBg
          e.currentTarget.style.color = baseColor
        }}
      >
        <Icon size={13} />
      </button>
      {badge && (
        <span
          style={{
            position: 'absolute',
            top: 2,
            right: 2,
            width: 5,
            height: 5,
            borderRadius: '50%',
            background: 'var(--color-primary)',
            pointerEvents: 'none',
          }}
        />
      )}
    </div>
  )
}

// @group Authentication > AuthGuard : Login gate + lock screen + inactivity timer
function AuthGuard() {
  const [authMode, setAuthMode] = useState<'checking' | 'disabled' | 'required' | 'error'>(
    'checking'
  )
  const [authed, setAuthed] = useState(isAuthenticated)
  const [locked, setLocked] = useState(false)
  const [lockConfig, setLockConfig] = useState<{
    pinConfigured: boolean
    lockTimeoutMins: number | null
  }>({
    pinConfigured: false,
    lockTimeoutMins: null,
  })
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  // @group Authentication > AuthGuard : Passwordless mode is server state,
  // not the presence of an old browser token. Resolve it before showing login.
  useEffect(() => {
    let cancelled = false

    function refreshAuthMode() {
      api
        .authStatus()
        .then(status => {
          if (cancelled) return
          if (status.password_configured) {
            setAuthMode('required')
            setAuthed(isAuthenticated())
          } else {
            clearSessionToken()
            setAuthed(true)
            setLocked(false)
            setAuthMode('disabled')
          }
        })
        .catch(() => {
          if (!cancelled) setAuthMode('error')
        })
    }

    refreshAuthMode()
    window.addEventListener('auth-config-updated', refreshAuthMode)
    return () => {
      cancelled = true
      window.removeEventListener('auth-config-updated', refreshAuthMode)
    }
  }, [])

  // @group Authentication > AuthGuard : Fetch lock config after authentication
  useEffect(() => {
    if (!authed || authMode !== 'required') return
    function fetchConfig() {
      api
        .authStatus()
        .then(s => {
          setLockConfig({
            pinConfigured: s.pin_configured ?? false,
            lockTimeoutMins: s.lock_timeout_mins ?? null,
          })
        })
        .catch(() => {})
    }
    fetchConfig()
    // Re-fetch when SettingsPage signals a change
    window.addEventListener('lock-config-updated', fetchConfig)
    return () => window.removeEventListener('lock-config-updated', fetchConfig)
  }, [authed, authMode])

  // @group Authentication > AuthGuard : Inactivity timer — lock after N minutes idle
  useEffect(() => {
    if (authMode !== 'required' || !authed || locked || !lockConfig.lockTimeoutMins) return
    const ms = lockConfig.lockTimeoutMins * 60 * 1000

    function resetTimer() {
      if (timerRef.current) clearTimeout(timerRef.current)
      timerRef.current = setTimeout(() => setLocked(true), ms)
    }
    const events = ['mousemove', 'keydown', 'click', 'scroll', 'touchstart'] as const
    events.forEach(e => window.addEventListener(e, resetTimer, { passive: true }))
    resetTimer()

    return () => {
      if (timerRef.current) clearTimeout(timerRef.current)
      events.forEach(e => window.removeEventListener(e, resetTimer))
    }
  }, [authMode, authed, locked, lockConfig])

  if (authMode === 'checking') {
    return <AuthStatusMessage message="正在检查访问设置…" />
  }
  if (authMode === 'error') {
    return (
      <AuthStatusMessage
        message="无法连接到 RunDock 守护进程。请确认服务已启动后刷新页面。"
        showRetry
      />
    )
  }

  if (authMode === 'required' && !authed) {
    return <LoginPage onAuthenticated={() => setAuthed(true)} />
  }
  if (authMode === 'required' && locked) {
    return <LoginPage onAuthenticated={() => setLocked(false)} subtitle="屏幕已锁定" />
  }
  return <Layout canLock={authMode === 'required'} onLock={() => setLocked(true)} />
}

function AuthStatusMessage({
  message,
  showRetry = false,
}: {
  message: string
  showRetry?: boolean
}) {
  return (
    <div
      style={{
        minHeight: '100vh',
        display: 'grid',
        placeItems: 'center',
        background: 'var(--color-background)',
        color: 'var(--color-foreground)',
      }}
    >
      <div style={{ textAlign: 'center', fontSize: 13 }}>
        <div>{message}</div>
        {showRetry && (
          <button
            onClick={() => window.location.reload()}
            style={{
              marginTop: 12,
              padding: '6px 14px',
              borderRadius: 6,
              border: '1px solid var(--color-border)',
              background: 'var(--color-secondary)',
              color: 'var(--color-foreground)',
              cursor: 'pointer',
            }}
          >
            重新连接
          </button>
        )}
      </div>
    </div>
  )
}

// @group Configuration > ReactQuery : Shared QueryClient — stale time 30s, no window-focus refetch
const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 30_000,
      refetchOnWindowFocus: false,
    },
  },
})

export default function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <BrowserRouter>
        <AuthGuard />
      </BrowserRouter>
      <GitHubStarBanner />
    </QueryClientProvider>
  )
}
