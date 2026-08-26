// @group BusinessLogic : Root app — layout shell + React Router

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { ReactQueryDevtoolsPanel } from '@tanstack/react-query-devtools'
import { lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { AuthGuard } from '@/components/AuthGuard'
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
  Clock,
  ScrollText,
  Settings,
  Bell,
  Network,
  BarChart2,
  Save,
  Lock,
  Power,
  Globe,
  Menu,
  Search,
} from 'lucide-react'
import { useDaemonHealth } from '@/hooks/useDaemonHealth'
import { useProcesses } from '@/hooks/useProcesses'
import { useProjects } from '@/hooks/useProjects'
import { useSettings } from '@/hooks/useSettings'
import { useDialog } from '@/hooks/useDialog'
import { useNotificationTray } from '@/hooks/useNotificationTray'
import { Dialog } from '@/components/Dialog'
import { GitHubStarBanner } from '@/components/GitHubStarBanner'
import { NotificationTray } from '@/components/NotificationTray'
import { AiPanel } from '@/components/AiPanel'
import { ServerSwitcher } from '@/components/ServerSwitcher'
import { StatusBar } from '@/components/StatusBar'
import { SystemStatsWidget } from '@/components/SystemStatsWidget'
import {
  CronJobSubmenu,
  LegacyNamespaceRedirect,
  NavBtn,
  NavRowWithAdd,
  SidebarProjectGroup,
} from '@/components/AppSidebar'
import {
  TerminalPanel,
  type TerminalPanelHandle,
  type TerminalPanelState,
  type TerminalShortcuts,
} from '@/components/TerminalPanel'
import { api } from '@/lib/api'
import type { ProjectInfo, UpdateInfo } from '@/types'

export { AuthGuard } from '@/components/AuthGuard'

const AnalyticsPage = lazy(() => import('@/pages/AnalyticsPage'))
const ProjectsPage = lazy(() => import('@/pages/ProjectsPage'))
const CronJobsPage = lazy(() => import('@/pages/CronJobsPage'))
const CreateCronJobPage = lazy(() => import('@/pages/CreateCronJobPage'))
const StartPage = lazy(() => import('@/pages/StartPage'))
const EditPage = lazy(() => import('@/pages/EditPage'))
const ProcessDetailPage = lazy(() => import('@/pages/ProcessDetailPage'))
const SettingsPage = lazy(() => import('@/pages/SettingsPage'))
const LogLibraryPage = lazy(() => import('@/pages/LogLibraryPage'))
const LogVolumePage = lazy(() => import('@/pages/LogVolumePage'))
const NotificationsPage = lazy(() => import('@/pages/NotificationsPage'))
const PortFinderPage = lazy(() => import('@/pages/PortFinderPage'))
const TunnelsPage = lazy(() => import('@/pages/TunnelsPage'))

// @group BusinessLogic > Layout : Sidebar + content shell
function Layout({ onLock, canLock }: { onLock: () => void; canLock: boolean }) {
  const { settings, updateSettings, resetToDefaults, error: settingsError } = useSettings()
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
  const {
    health,
    error: healthError,
    warning: healthWarning,
  } = useDaemonHealth(settings.healthRefreshInterval)
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

  const connected = health !== null && error === null && healthError === null
  const operationalError = settingsError ?? error ?? projectsError ?? healthError ?? healthWarning

  // @group BusinessLogic > Update : Check for new version once on initial load
  const [updateInfo, setUpdateInfo] = useState<UpdateInfo | null>(null)
  useEffect(() => {
    api
      .checkUpdate()
      .then(info => {
        if (!info.up_to_date && !info.error) setUpdateInfo(info)
      })
      .catch(error => console.warn('Update check failed', error))
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
    try {
      await api.saveState()
      await alert('状态已保存', '进程状态已保存到磁盘。')
    } catch (saveError) {
      await alert(
        '保存失败',
        saveError instanceof Error ? saveError.message : '进程状态未能保存到磁盘。'
      )
    }
  }

  async function handleShutdown() {
    if (settings.confirmBeforeShutdown) {
      const ok = await confirm('关闭守护进程？', 'RunDock 守护进程将停止，受管理的进程会继续运行。')
      if (!ok) return
    }
    try {
      await api.shutdownDaemon()
    } catch (shutdownError) {
      await alert(
        '关闭失败',
        shutdownError instanceof Error ? shutdownError.message : '守护进程拒绝关闭。'
      )
    }
  }

  const isProcessActive =
    location.pathname === '/processes' || location.pathname.startsWith('/processes/')
  const isCronActive = location.pathname === '/cron-jobs' || location.pathname === '/cron-jobs/new'
  const isPortsActive = location.pathname === '/ports'
  const isTunnelsActive = location.pathname === '/tunnels'

  const [cronOpen, setCronOpen] = useState(false)
  const cronJobs = useMemo(() => processes.filter(p => p.cron), [processes])
  const currentCronNamespace =
    location.pathname === '/cron-jobs'
      ? new URLSearchParams(location.search).get('namespace')
      : null
  const [toolsOpen, setToolsOpen] = useState(false)
  const [sidebarOpen, setSidebarOpen] = useState(false)
  const [compactSidebar, setCompactSidebar] = useState(
    () => window.matchMedia?.('(max-width: 760px)').matches ?? false
  )
  const sidebarRef = useRef<HTMLElement>(null)

  useEffect(() => {
    if (!window.matchMedia) return
    const query = window.matchMedia('(max-width: 760px)')
    const update = () => setCompactSidebar(query.matches)
    update()
    query.addEventListener('change', update)
    return () => query.removeEventListener('change', update)
  }, [])

  useEffect(() => {
    if (!compactSidebar || !sidebarOpen) return
    const previous = document.activeElement as HTMLElement | null
    const focusTimer = window.setTimeout(() => {
      sidebarRef.current?.querySelector<HTMLElement>('a, button:not([disabled])')?.focus()
    }, 0)
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.preventDefault()
        setSidebarOpen(false)
        return
      }
      if (event.key !== 'Tab') return
      const focusable = sidebarRef.current?.querySelectorAll<HTMLElement>(
        'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])'
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
    window.addEventListener('keydown', handleKeyDown)
    return () => {
      window.clearTimeout(focusTimer)
      window.removeEventListener('keydown', handleKeyDown)
      if (previous?.isConnected) previous.focus()
    }
  }, [compactSidebar, sidebarOpen])
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
          ref={sidebarRef}
          className="rundock-sidebar"
          data-open={sidebarOpen}
          aria-hidden={compactSidebar && !sidebarOpen}
          inert={compactSidebar && !sidebarOpen}
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
            <CronJobSubmenu
              processes={processes}
              currentNamespace={currentCronNamespace}
              open={cronOpen}
            />

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
              type="button"
              onClick={() => setToolsOpen(v => !v)}
              aria-expanded={toolsOpen}
              aria-controls="sidebar-tools"
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
              <div id="sidebar-tools">
                <NavBtn to="/ports" icon={Network} label="端口查找" active={isPortsActive} />
                <NavBtn to="/tunnels" icon={Globe} label="隧道" active={isTunnelsActive} />
              </div>
            )}
          </nav>

          {/* Active projects list */}
          <div className="rundock-sidebar-content">
            <div style={{ padding: '6px 8px 4px', flexShrink: 0 }}>
              <input
                aria-label="筛选项目"
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
                      try {
                        const response = await api.stopProject(project.id)
                        const failure = response.results.find(result => !result.success)
                        if (failure)
                          throw new Error(`${failure.name}：${failure.error ?? '停止失败'}`)
                      } finally {
                        reload()
                      }
                    }}
                    onRestart={async project => {
                      try {
                        const response = await api.restartProject(project.id)
                        const failure = response.results.find(result => !result.success)
                        if (failure)
                          throw new Error(`${failure.name}：${failure.error ?? '重启失败'}`)
                      } finally {
                        reload()
                      }
                    }}
                    onError={message => void alert('项目操作失败', message)}
                  />
                ))
              )}
            </div>
          </div>

          {/* Footer */}
          <div className="rundock-sidebar-footer">
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
        <section
          className="rundock-workspace"
          aria-hidden={compactSidebar && sidebarOpen}
          inert={compactSidebar && sidebarOpen}
        >
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
            {operationalError && (
              <div
                role="alert"
                style={{
                  padding: '8px 14px',
                  color: 'var(--color-status-crashed)',
                  background: 'color-mix(in srgb, var(--color-status-crashed) 10%, transparent)',
                  borderBottom: '1px solid var(--color-status-crashed)',
                  fontSize: 12,
                }}
              >
                {operationalError}；页面保留上次成功数据，恢复连接后会自动刷新。
              </div>
            )}
            <Suspense fallback={<div className="rundock-route-loading">页面加载中…</div>}>
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
                <Route path="/namespace/:name" element={<LegacyNamespaceRedirect />} />
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
                  element={
                    <CronJobsPage
                      processes={processes}
                      reload={reload}
                      settings={settings}
                      namespaceFilter={currentCronNamespace}
                    />
                  }
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
                <Route path="*" element={<Navigate to="/" replace />} />
              </Routes>
            </Suspense>
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
  const serverSwitcher = (
    <div
      style={{
        position: 'fixed',
        left: 12,
        bottom: 12,
        width: 260,
        zIndex: 1000,
      }}
      aria-label="服务器恢复入口"
    >
      <ServerSwitcher />
    </div>
  )
  return (
    <QueryClientProvider client={queryClient}>
      <BrowserRouter>
        <AuthGuard recovery={serverSwitcher}>
          {({ canLock, onLock }) => (
            <>
              {serverSwitcher}
              <Layout canLock={canLock} onLock={onLock} />
            </>
          )}
        </AuthGuard>
      </BrowserRouter>
      <GitHubStarBanner />
    </QueryClientProvider>
  )
}
