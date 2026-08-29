// @group BusinessLogic : Bottom status bar and operational panel toggles

import type React from 'react'
import { BarChart2, Bell, Bot } from 'lucide-react'
import { DiscordIcon } from '@/components/DiscordIcon'
import { GitHubStarWidget } from '@/components/GitHubStarBanner'
import { TerminalStatusBarBtn, type TerminalPanelState } from '@/components/TerminalPanel'
import { getActiveServer, type RemoteServer } from '@/lib/servers'
import type { ProjectInfo, UpdateInfo } from '@/types'

// @group BusinessLogic > StatusBar > Menu : Overflow dropdown for status bar actions
// @group BusinessLogic > StatusBar : VSCode-style status bar — fixed at viewport bottom
export function StatusBar({
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
  let activeServer: RemoteServer | null = null
  let activeServerError = false
  try {
    activeServer = getActiveServer()
  } catch {
    activeServerError = true
  }
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
        <span role={activeServerError ? 'alert' : undefined}>
          {activeServerError
            ? '服务器配置错误'
            : activeServer?.id === 'local'
              ? '本地'
              : activeServer?.name}
        </span>
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
      {(updateInfo || version) && (
        <button
          type="button"
          onClick={updateInfo ? onGoToUpdate : undefined}
          aria-label={updateInfo ? `更新到版本 ${updateInfo.latest}` : `当前版本 ${version}`}
          title={
            updateInfo
              ? `有可用更新：v${updateInfo.latest} — 点击前往设置`
              : version
                ? `RunDock v${version}`
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
          ) : (
            <span style={{ opacity: 0.55 }}>v{version}</span>
          )}
        </button>
      )}

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
        type="button"
        onClick={onToggleTray}
        aria-label={unreadCount > 0 ? `通知，${unreadCount} 条未读` : '通知'}
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
        type="button"
        onClick={onToggleAi}
        aria-label="AI 助手"
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
        type="button"
        onClick={onToggleStats}
        aria-label="系统统计"
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
          type="button"
          onClick={onToggleDevtools}
          aria-label="React Query 开发工具"
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
