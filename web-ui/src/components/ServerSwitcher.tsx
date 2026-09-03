// @group BusinessLogic : Remote daemon server switching and SSH tunnel setup

import { useEffect, useRef, useState } from 'react'
import type React from 'react'
import { Plus, Server } from 'lucide-react'
import {
  getActiveServerId,
  getServers,
  isLoopbackHost,
  LOCAL_SERVER,
  normalizeServerHost,
  resetServers,
  saveServers,
  setActiveServerId,
  sshTunnelCommand,
  type RemoteServer,
} from '@/lib/servers'

interface ServerFormState {
  name: string
  host: string
  port: string
  sshHost: string
  sshPort: string
  sshUser: string
  sshKeyPath: string
  remoteDaemonPort: string
  localPort: string
}

function loadInitialServerState(): {
  remotes: RemoteServer[]
  activeId: string
  error: string | null
} {
  try {
    const remotes = getServers()
    const activeId = getActiveServerId()
    if (activeId !== 'local' && !remotes.some(server => server.id === activeId)) {
      throw new Error('当前活动服务器配置已不存在')
    }
    return { remotes, activeId, error: null }
  } catch (error) {
    return {
      remotes: [],
      activeId: 'local',
      error: error instanceof Error ? error.message : '无法读取服务器配置',
    }
  }
}

function parseServerPort(value: string): number | null {
  const port = Number(value)
  return Number.isInteger(port) && port >= 1 && port <= 65535 ? port : null
}

function sshServerFromForm(form: ServerFormState, id: string, name: string): RemoteServer | null {
  const localPort = parseServerPort(form.localPort)
  const sshPort = parseServerPort(form.sshPort)
  const remoteDaemonPort = parseServerPort(form.remoteDaemonPort)
  const sshHost = normalizeServerHost(form.sshHost)
  if (!sshHost || !form.sshUser.trim() || !localPort || !sshPort || !remoteDaemonPort) {
    return null
  }
  return {
    id,
    name,
    host: '127.0.0.1',
    port: localPort,
    connectionType: 'ssh',
    sshHost,
    sshPort,
    sshUser: form.sshUser.trim(),
    sshKeyPath: form.sshKeyPath.trim() || undefined,
    remoteDaemonPort,
  }
}

interface ServerSwitcherProps {
  variant?: 'popover' | 'settings'
}

// @group BusinessLogic > ServerSwitcher : Local/remote daemon selection and management
export function ServerSwitcher({ variant = 'popover' }: ServerSwitcherProps) {
  const embedded = variant === 'settings'
  const [initialState] = useState(loadInitialServerState)
  const [open, setOpen] = useState(false)
  const [remotes, setRemotes] = useState<RemoteServer[]>(initialState.remotes)
  const [activeId, setActiveId] = useState(initialState.activeId)
  const [storageError, setStorageError] = useState<string | null>(initialState.error)
  const [addMode, setAddMode] = useState(false)
  const [connType, setConnType] = useState<'direct' | 'ssh'>('direct')
  const [form, setForm] = useState<ServerFormState>({
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
  const triggerRef = useRef<HTMLButtonElement>(null)
  const panelRef = useRef<HTMLDivElement>(null)
  const activeServer =
    activeId === 'local' ? LOCAL_SERVER : (remotes.find(s => s.id === activeId) ?? LOCAL_SERVER)
  const sshPreview = sshServerFromForm(form, 'preview', '')

  useEffect(() => {
    if (!open) return
    const focusTimer = window.setTimeout(() => {
      panelRef.current
        ?.querySelector<HTMLElement>('button:not([disabled]), input:not([disabled])')
        ?.focus()
    }, 0)
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return
      event.preventDefault()
      setOpen(false)
      triggerRef.current?.focus()
    }
    window.addEventListener('keydown', handleKeyDown)
    return () => {
      window.clearTimeout(focusTimer)
      window.removeEventListener('keydown', handleKeyDown)
    }
  }, [open])

  function switchTo(id: string) {
    try {
      setActiveServerId(id)
      window.location.reload()
    } catch (error) {
      setStorageError(error instanceof Error ? error.message : '无法保存活动服务器')
    }
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
      const host = normalizeServerHost(form.host)
      if (!host) {
        setFormError('主机名或 IP 地址无效，请勿包含协议、路径或空格')
        return
      }
      const port = parseServerPort(form.port)
      if (port === null) {
        setFormError('端口无效')
        return
      }
      newServer = {
        id: crypto.randomUUID(),
        name: form.name.trim(),
        host,
        port,
        connectionType: 'direct',
        protocol: isLoopbackHost(host) ? 'http' : 'https',
      }
    } else {
      if (!sshPreview) {
        setFormError('SSH 配置无效，请检查主机、用户名和端口，且主机中不要包含协议、路径或空格')
        return
      }
      if (!form.sshUser.trim()) {
        setFormError('SSH 用户名不能为空')
        return
      }
      const localPort = parseServerPort(form.localPort)
      if (localPort === null) {
        setFormError('本地端口无效')
        return
      }
      const sshPort = parseServerPort(form.sshPort)
      if (sshPort === null) {
        setFormError('SSH 端口无效')
        return
      }
      const remoteDaemonPort = parseServerPort(form.remoteDaemonPort)
      if (remoteDaemonPort === null) {
        setFormError('远端守护进程端口无效')
        return
      }
      newServer = {
        ...sshPreview,
        id: crypto.randomUUID(),
        name: form.name.trim(),
      }
    }
    const updated = [...remotes, newServer]
    try {
      saveServers(updated)
      setRemotes(updated)
      resetForm()
      setAddMode(false)
      setStorageError(null)
    } catch (error) {
      setStorageError(error instanceof Error ? error.message : '无法保存服务器配置')
    }
  }

  function removeServer(id: string) {
    const updated = remotes.filter(s => s.id !== id)
    try {
      saveServers(updated)
      setRemotes(updated)
      if (activeId === id) {
        setActiveServerId('local')
        setActiveId('local')
        window.location.reload()
      }
      setStorageError(null)
    } catch (error) {
      setStorageError(error instanceof Error ? error.message : '无法更新服务器配置')
    }
  }

  function resetStoredServers() {
    try {
      resetServers()
      setRemotes([])
      setActiveId('local')
      setStorageError(null)
      resetForm()
    } catch (error) {
      setStorageError(error instanceof Error ? error.message : '无法重置服务器配置')
    }
  }

  async function copyTunnelCmd() {
    if (!sshPreview) {
      setFormError('请先填写有效的 SSH 主机、用户名和端口')
      return
    }
    try {
      if (!navigator.clipboard?.writeText) throw new Error('当前环境不支持剪贴板')
      await navigator.clipboard.writeText(sshTunnelCommand(sshPreview))
      setCopiedCmd(true)
      setTimeout(() => setCopiedCmd(false), 2000)
    } catch (error) {
      setFormError(error instanceof Error ? `复制失败：${error.message}` : '复制失败')
    }
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
      {!embedded && (
        <button
          ref={triggerRef}
          type="button"
          onClick={() => setOpen(v => !v)}
          aria-expanded={open}
          aria-controls="server-switcher-panel"
          aria-label={`当前服务器：${activeServer.name}，点击切换服务器`}
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
              ? '本机'
              : activeServer.connectionType === 'ssh'
                ? `SSH：${activeServer.sshHost}`
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
      )}

      {/* Dropdown panel */}
      {(embedded || open) && (
        <div
          ref={panelRef}
          id="server-switcher-panel"
          role={embedded ? 'region' : 'dialog'}
          aria-modal={embedded ? undefined : 'false'}
          aria-label={embedded ? '服务器连接设置' : '服务器切换'}
          style={
            embedded
              ? {
                  background: 'var(--color-card)',
                  border: '1px solid var(--color-border)',
                  borderRadius: 8,
                  overflow: 'hidden',
                  padding: '6px 0',
                }
              : {
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
                }
          }
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

          {storageError && (
            <div role="alert" style={{ padding: '4px 10px 8px', fontSize: 10 }}>
              <div style={{ color: 'var(--color-destructive)', marginBottom: 5 }}>
                {storageError}
              </div>
              <button type="button" onClick={resetStoredServers} style={addBtnStyle}>
                重置为本地服务器
              </button>
            </div>
          )}

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
                {s.id === activeId && (
                  <span
                    style={{
                      padding: '1px 5px',
                      borderRadius: 7,
                      background: 'var(--color-primary)',
                      color: 'var(--color-primary-foreground)',
                      fontSize: 9,
                      lineHeight: 1.4,
                      flexShrink: 0,
                    }}
                  >
                    当前
                  </span>
                )}
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
                <div role="alert" style={{ fontSize: 10, color: 'var(--color-destructive)' }}>
                  {formError}
                </div>
              )}

              <input
                aria-label="服务器名称"
                value={form.name}
                onChange={e => setForm(f => ({ ...f, name: e.target.value }))}
                placeholder="名称（例如：生产环境）"
                style={addInputStyle}
              />

              {connType === 'direct' ? (
                <>
                  <input
                    aria-label="直连主机"
                    value={form.host}
                    onChange={e => setForm(f => ({ ...f, host: e.target.value }))}
                    placeholder="主机（非本机默认 HTTPS）"
                    style={addInputStyle}
                  />
                  <input
                    aria-label="守护进程端口"
                    value={form.port}
                    onChange={e => setForm(f => ({ ...f, port: e.target.value }))}
                    placeholder="守护进程端口（2999）"
                    style={addInputStyle}
                  />
                </>
              ) : (
                <>
                  <input
                    aria-label="SSH 主机"
                    value={form.sshHost}
                    onChange={e => setForm(f => ({ ...f, sshHost: e.target.value }))}
                    placeholder="SSH 主机（例如：myserver.com）"
                    style={addInputStyle}
                  />
                  <div style={{ display: 'flex', gap: 4 }}>
                    <input
                      aria-label="SSH 用户名"
                      value={form.sshUser}
                      onChange={e => setForm(f => ({ ...f, sshUser: e.target.value }))}
                      placeholder="用户名"
                      style={{ ...addInputStyle, flex: 1 }}
                    />
                    <input
                      aria-label="SSH 端口"
                      value={form.sshPort}
                      onChange={e => setForm(f => ({ ...f, sshPort: e.target.value }))}
                      placeholder="SSH 端口"
                      style={{ ...addInputStyle, width: 72 }}
                    />
                  </div>
                  <input
                    aria-label="SSH 密钥路径"
                    value={form.sshKeyPath}
                    onChange={e => setForm(f => ({ ...f, sshKeyPath: e.target.value }))}
                    placeholder="SSH 密钥路径（可选，例如：~/.ssh/id_rsa）"
                    style={addInputStyle}
                  />
                  <div style={{ display: 'flex', gap: 4 }}>
                    <input
                      aria-label="远程守护进程端口"
                      value={form.remoteDaemonPort}
                      onChange={e => setForm(f => ({ ...f, remoteDaemonPort: e.target.value }))}
                      placeholder="远程端口"
                      style={{ ...addInputStyle, flex: 1 }}
                    />
                    <input
                      aria-label="本地转发端口"
                      value={form.localPort}
                      onChange={e => setForm(f => ({ ...f, localPort: e.target.value }))}
                      placeholder="本地转发端口"
                      style={{ ...addInputStyle, flex: 1 }}
                    />
                  </div>
                  {/* SSH tunnel command preview */}
                  {sshPreview && (
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
                        {sshTunnelCommand(sshPreview)}
                      </code>
                      <button
                        type="button"
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
