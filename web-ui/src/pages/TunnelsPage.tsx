// @group BusinessLogic : Tunnels page — create and manage cloudflared / ngrok / custom tunnels

import { useCallback, useEffect, useRef, useState } from 'react'
import { Copy, Check, ExternalLink, RefreshCw, Plus, Square, Trash2, Globe } from 'lucide-react'
import { api } from '@/lib/api'
import { useSingleFlightPoll } from '@/hooks/useSingleFlightPoll'
import type { TunnelEntry, TunnelProvider, TunnelStatus } from '@/types'

// @group Utilities > Styles : Shared style tokens
const card: React.CSSProperties = {
  background: 'var(--color-card)',
  border: '1px solid var(--color-border)',
  borderRadius: 8,
  padding: '16px 20px',
  marginBottom: 12,
}

const inputStyle: React.CSSProperties = {
  padding: '6px 10px',
  fontSize: 13,
  background: 'var(--color-secondary)',
  border: '1px solid var(--color-border)',
  borderRadius: 6,
  color: 'var(--color-foreground)',
  outline: 'none',
  width: '100%',
  boxSizing: 'border-box' as const,
}

const btnPrimary: React.CSSProperties = {
  padding: '7px 16px',
  fontSize: 13,
  fontWeight: 500,
  background: 'var(--color-primary)',
  color: '#fff',
  border: 'none',
  borderRadius: 6,
  cursor: 'pointer',
}

// @group Utilities > TunnelsPage : Status badge colour
function statusColor(s: TunnelStatus): string {
  switch (s) {
    case 'active':
      return 'var(--color-status-running)'
    case 'starting':
      return 'var(--color-status-sleeping)'
    case 'failed':
      return 'var(--color-status-crashed)'
    case 'stopped':
      return 'var(--color-muted-foreground)'
  }
}

// @group Utilities > TunnelsPage : Provider display name + colour
function providerLabel(p: TunnelProvider): { name: string; color: string } {
  switch (p) {
    case 'cloudflare':
      return { name: 'Cloudflare', color: '#f48120' }
    case 'ngrok':
      return { name: 'ngrok', color: '#1f2d3d' }
    case 'custom':
      return { name: '自定义', color: 'var(--color-muted-foreground)' }
  }
}

// @group Utilities > TunnelsPage : Copy-to-clipboard button
function CopyBtn({ text }: { text: string }) {
  const [copied, setCopied] = useState(false)
  const [copyFailed, setCopyFailed] = useState(false)
  return (
    <button
      type="button"
      aria-label={
        copyFailed ? '复制隧道 URL 失败，请重试' : copied ? '已复制隧道 URL' : '复制隧道 URL'
      }
      onClick={async () => {
        setCopyFailed(false)
        try {
          await navigator.clipboard.writeText(text)
          setCopied(true)
          setTimeout(() => setCopied(false), 1800)
        } catch {
          setCopyFailed(true)
        }
      }}
      title={copyFailed ? '复制失败，请检查剪贴板权限' : '复制 URL'}
      style={{
        background: 'transparent',
        border: 'none',
        cursor: 'pointer',
        padding: 3,
        color: copyFailed
          ? 'var(--color-destructive)'
          : copied
            ? 'var(--color-status-running)'
            : 'var(--color-muted-foreground)',
        display: 'flex',
        alignItems: 'center',
      }}
    >
      {copied ? <Check size={13} /> : <Copy size={13} />}
    </button>
  )
}

// @group BusinessLogic > TunnelsPage : Single tunnel row
function TunnelRow({
  tunnel,
  onStop,
  onRemove,
  busy,
}: {
  tunnel: TunnelEntry
  onStop: () => void
  onRemove: () => void
  busy: boolean
}) {
  const prov = providerLabel(tunnel.provider)
  const isLive = tunnel.status === 'active' || tunnel.status === 'starting'

  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: 12,
        padding: '12px 0',
        borderBottom: '1px solid var(--color-border)',
      }}
    >
      {/* Provider badge */}
      <span
        style={{
          fontSize: 10,
          fontWeight: 700,
          letterSpacing: '0.06em',
          padding: '2px 7px',
          borderRadius: 10,
          background: prov.color + '22',
          color: prov.color,
          flexShrink: 0,
        }}
      >
        {prov.name}
      </span>

      {/* Port */}
      <span
        style={{
          fontSize: 13,
          fontWeight: 600,
          color: 'var(--color-foreground)',
          flexShrink: 0,
          minWidth: 48,
        }}
      >
        :{tunnel.port}
      </span>

      {/* Process name */}
      {tunnel.process_name && (
        <span
          style={{
            fontSize: 12,
            color: 'var(--color-muted-foreground)',
            flexShrink: 0,
            maxWidth: 120,
            overflow: 'hidden',
            textOverflow: 'ellipsis',
            whiteSpace: 'nowrap',
          }}
        >
          {tunnel.process_name}
        </span>
      )}

      {/* Public URL or status */}
      <div style={{ flex: 1, minWidth: 0 }}>
        {tunnel.public_url ? (
          <div style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
            <a
              href={tunnel.public_url}
              target="_blank"
              rel="noopener noreferrer"
              aria-label="在新窗口打开隧道 URL"
              title="在新窗口打开隧道 URL"
              style={{
                fontSize: 13,
                color: 'var(--color-primary)',
                textDecoration: 'none',
                overflow: 'hidden',
                textOverflow: 'ellipsis',
                whiteSpace: 'nowrap',
                maxWidth: 340,
              }}
            >
              {tunnel.public_url}
            </a>
            <CopyBtn text={tunnel.public_url} />
            <a
              href={tunnel.public_url}
              target="_blank"
              rel="noopener noreferrer"
              aria-label="在新窗口打开隧道 URL"
              title="在新窗口打开隧道 URL"
              style={{
                color: 'var(--color-muted-foreground)',
                display: 'flex',
                alignItems: 'center',
              }}
            >
              <ExternalLink size={12} />
            </a>
          </div>
        ) : tunnel.error ? (
          <span style={{ fontSize: 12, color: 'var(--color-status-crashed)' }}>{tunnel.error}</span>
        ) : (
          <span
            style={{ fontSize: 12, color: 'var(--color-muted-foreground)', fontStyle: 'italic' }}
          >
            {tunnel.status === 'starting' ? '等待 URL…' : '—'}
          </span>
        )}
      </div>

      {/* Status dot */}
      <span
        style={{
          display: 'inline-flex',
          alignItems: 'center',
          gap: 4,
          fontSize: 11,
          color: statusColor(tunnel.status),
          flexShrink: 0,
        }}
      >
        <span
          style={{
            width: 7,
            height: 7,
            borderRadius: '50%',
            background: statusColor(tunnel.status),
            display: 'inline-block',
          }}
        />
        {tunnel.status === 'active'
          ? '活动'
          : tunnel.status === 'starting'
            ? '启动中'
            : tunnel.status === 'failed'
              ? '失败'
              : '已停止'}
      </span>

      {/* Actions */}
      {isLive ? (
        <button
          onClick={onStop}
          disabled={busy}
          aria-busy={busy}
          title="停止隧道"
          style={{
            background: 'transparent',
            border: '1px solid var(--color-border)',
            borderRadius: 5,
            padding: '4px 8px',
            cursor: busy ? 'wait' : 'pointer',
            opacity: busy ? 0.65 : 1,
            color: 'var(--color-muted-foreground)',
            display: 'flex',
            alignItems: 'center',
            gap: 4,
            fontSize: 12,
            flexShrink: 0,
          }}
        >
          <Square size={11} /> {busy ? '处理中…' : '停止'}
        </button>
      ) : (
        <button
          onClick={onRemove}
          disabled={busy}
          aria-busy={busy}
          title="移除"
          style={{
            background: 'transparent',
            border: '1px solid var(--color-border)',
            borderRadius: 5,
            padding: '4px 8px',
            cursor: busy ? 'wait' : 'pointer',
            opacity: busy ? 0.65 : 1,
            color: 'var(--color-status-crashed)',
            display: 'flex',
            alignItems: 'center',
            gap: 4,
            fontSize: 12,
            flexShrink: 0,
          }}
        >
          <Trash2 size={11} /> {busy ? '处理中…' : '移除'}
        </button>
      )}
    </div>
  )
}

// @group BusinessLogic > TunnelsPage : Inline form to create a new tunnel
function CreateForm({
  defaultProvider,
  onCreated,
  onCancel,
}: {
  defaultProvider: TunnelProvider
  onCreated: () => void
  onCancel: () => void
}) {
  const [port, setPort] = useState('')
  const [procName, setProcName] = useState('')
  const [provider, setProvider] = useState<TunnelProvider>(defaultProvider)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  async function submit(e: React.FormEvent) {
    e.preventDefault()
    const portNum = parseInt(port, 10)
    if (!portNum || portNum < 1 || portNum > 65535) {
      setError('请输入有效端口（1–65535）')
      return
    }
    setBusy(true)
    setError(null)
    try {
      const res = await api.createTunnel({
        port: portNum,
        process_name: procName || null,
        provider,
      })
      if (res.error) {
        setError(res.error)
        return
      }
      if (!res.tunnel) {
        setError('服务端未返回已创建的隧道')
        return
      }
      onCreated()
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : '创建隧道失败')
    } finally {
      setBusy(false)
    }
  }

  return (
    <form onSubmit={submit} style={{ ...card, marginBottom: 16 }}>
      <p
        style={{
          fontSize: 11,
          fontWeight: 700,
          letterSpacing: '0.08em',
          color: 'var(--color-muted-foreground)',
          textTransform: 'uppercase',
          marginBottom: 14,
          marginTop: 0,
        }}
      >
        新建隧道
      </p>
      <div style={{ display: 'flex', gap: 10, flexWrap: 'wrap', alignItems: 'flex-end' }}>
        <label style={{ display: 'flex', flexDirection: 'column', gap: 4, flex: '0 0 120px' }}>
          <span style={{ fontSize: 11, color: 'var(--color-muted-foreground)' }}>端口 *</span>
          <input
            type="number"
            min={1}
            max={65535}
            placeholder="3000"
            value={port}
            onChange={e => setPort(e.target.value)}
            style={{ ...inputStyle, width: 120 }}
            autoFocus
          />
        </label>

        <label style={{ display: 'flex', flexDirection: 'column', gap: 4, flex: '0 0 160px' }}>
          <span style={{ fontSize: 11, color: 'var(--color-muted-foreground)' }}>
            进程名称（可选）
          </span>
          <input
            type="text"
            placeholder="my-app"
            value={procName}
            onChange={e => setProcName(e.target.value)}
            style={{ ...inputStyle, width: 160 }}
          />
        </label>

        <label style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
          <span style={{ fontSize: 11, color: 'var(--color-muted-foreground)' }}>提供商</span>
          <select
            value={provider}
            onChange={e => setProvider(e.target.value as TunnelProvider)}
            style={{ ...inputStyle, width: 140 }}
          >
            <option value="cloudflare">Cloudflare</option>
            <option value="ngrok">ngrok</option>
            <option value="custom">自定义</option>
          </select>
        </label>

        <div style={{ display: 'flex', gap: 8, paddingBottom: 1 }}>
          <button type="submit" disabled={busy} style={{ ...btnPrimary, opacity: busy ? 0.6 : 1 }}>
            {busy ? '创建中…' : '创建'}
          </button>
          <button
            type="button"
            onClick={onCancel}
            style={{
              padding: '7px 14px',
              fontSize: 13,
              background: 'transparent',
              border: '1px solid var(--color-border)',
              borderRadius: 6,
              cursor: 'pointer',
              color: 'var(--color-foreground)',
            }}
          >
            取消
          </button>
        </div>
      </div>
      {error && (
        <p
          style={{
            fontSize: 12,
            color: 'var(--color-status-crashed)',
            marginTop: 10,
            marginBottom: 0,
          }}
        >
          {error}
        </p>
      )}
    </form>
  )
}

// @group BusinessLogic > TunnelsPage : Main page component
export default function TunnelsPage() {
  const [tunnels, setTunnels] = useState<TunnelEntry[]>([])
  const [loading, setLoading] = useState(true)
  const [listError, setListError] = useState<string | null>(null)
  const [settingsError, setSettingsError] = useState<string | null>(null)
  const [actionError, setActionError] = useState<string | null>(null)
  const [pollError, setPollError] = useState<string | null>(null)
  const [showForm, setShowForm] = useState(false)
  const [defaultProvider, setDefaultProvider] = useState<TunnelProvider>('cloudflare')
  const [startingPollTimedOut, setStartingPollTimedOut] = useState(false)
  const [startingPollEpoch, setStartingPollEpoch] = useState(0)
  const [busyTunnelIds, setBusyTunnelIds] = useState<Set<string>>(new Set())
  const busyTunnelIdsRef = useRef<Set<string>>(new Set())
  const loadedRef = useRef(false)

  const load = useCallback(async (isCurrent: () => boolean, signal: AbortSignal) => {
    if (isCurrent() && !loadedRef.current) setLoading(true)
    try {
      const data = await api.getTunnels({ signal })
      if (isCurrent()) {
        setTunnels(data.tunnels ?? [])
        setListError(null)
      }
    } catch (e: unknown) {
      if (isCurrent()) setListError(e instanceof Error ? e.message : '加载隧道失败')
      throw e
    } finally {
      if (isCurrent()) {
        loadedRef.current = true
        setLoading(false)
      }
    }
  }, [])

  // Load default provider from settings
  useEffect(() => {
    api
      .getTunnelSettings()
      .then(s => {
        setDefaultProvider(s.provider)
        setSettingsError(null)
      })
      .catch(loadError => {
        setSettingsError(loadError instanceof Error ? loadError.message : '加载隧道设置失败')
      })
  }, [])

  // Poll while any tunnel is in "starting" state
  const hasStarting = tunnels.some(t => t.status === 'starting')
  const startingKey = tunnels
    .filter(t => t.status === 'starting')
    .map(t => t.id)
    .sort((left, right) => left.localeCompare(right))
    .join(',')
  useEffect(() => {
    setStartingPollTimedOut(false)
    setPollError(null)
    if (!startingKey) return
    const timer = window.setTimeout(() => {
      setStartingPollTimedOut(true)
      setPollError('隧道启动等待已超过 2 分钟，已暂停自动轮询；请检查供应商状态后手动刷新')
    }, 120_000)
    return () => window.clearTimeout(timer)
  }, [startingKey, startingPollEpoch])
  const reloadTunnels = useSingleFlightPoll(load, {
    intervalMs: 2000,
    enabled: hasStarting && !startingPollTimedOut,
  })

  useEffect(() => {
    void reloadTunnels()
  }, [reloadTunnels])

  function handleManualReload() {
    setStartingPollTimedOut(false)
    setPollError(null)
    setStartingPollEpoch(epoch => epoch + 1)
    void reloadTunnels()
  }

  async function handleStop(id: string) {
    if (busyTunnelIdsRef.current.has(id)) return
    busyTunnelIdsRef.current.add(id)
    setBusyTunnelIds(new Set(busyTunnelIdsRef.current))
    setActionError(null)
    try {
      const result = await api.stopTunnel(id)
      if (!result.success) throw new Error(result.error ?? '停止隧道失败')
      await reloadTunnels()
    } catch (actionError) {
      setActionError(actionError instanceof Error ? actionError.message : '停止隧道失败')
    } finally {
      busyTunnelIdsRef.current.delete(id)
      setBusyTunnelIds(new Set(busyTunnelIdsRef.current))
    }
  }

  async function handleRemove(id: string) {
    if (busyTunnelIdsRef.current.has(id)) return
    busyTunnelIdsRef.current.add(id)
    setBusyTunnelIds(new Set(busyTunnelIdsRef.current))
    setActionError(null)
    try {
      const result = await api.removeTunnel(id)
      if (!result.success) throw new Error(result.error ?? '移除隧道失败')
      await reloadTunnels()
    } catch (actionError) {
      setActionError(actionError instanceof Error ? actionError.message : '移除隧道失败')
    } finally {
      busyTunnelIdsRef.current.delete(id)
      setBusyTunnelIds(new Set(busyTunnelIdsRef.current))
    }
  }

  const activeTunnels = tunnels.filter(t => t.status === 'active' || t.status === 'starting')
  const inactiveTunnels = tunnels.filter(t => t.status === 'stopped' || t.status === 'failed')
  const error = actionError ?? pollError ?? listError ?? settingsError

  return (
    <div style={{ padding: '24px 28px', maxWidth: 860, fontFamily: 'inherit' }}>
      {/* Header */}
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          marginBottom: 20,
        }}
      >
        <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
          <Globe size={18} style={{ color: 'var(--color-primary)' }} />
          <h1
            style={{ fontSize: 18, fontWeight: 700, margin: 0, color: 'var(--color-foreground)' }}
          >
            隧道
          </h1>
          {activeTunnels.length > 0 && (
            <span
              style={{
                fontSize: 11,
                fontWeight: 600,
                background: 'var(--color-status-running)',
                color: '#fff',
                borderRadius: 10,
                padding: '1px 8px',
              }}
            >
              {activeTunnels.length} 个活动
            </span>
          )}
        </div>
        <div style={{ display: 'flex', gap: 8 }}>
          <button
            type="button"
            onClick={handleManualReload}
            title="刷新"
            aria-label="刷新隧道列表"
            style={{
              background: 'transparent',
              border: '1px solid var(--color-border)',
              borderRadius: 6,
              padding: '6px 10px',
              cursor: 'pointer',
              color: 'var(--color-muted-foreground)',
              display: 'flex',
              alignItems: 'center',
            }}
          >
            <RefreshCw size={13} />
          </button>
          {!showForm && (
            <button
              onClick={() => setShowForm(true)}
              style={{ ...btnPrimary, display: 'flex', alignItems: 'center', gap: 6 }}
            >
              <Plus size={14} /> 新建隧道
            </button>
          )}
        </div>
      </div>

      <p
        style={{
          fontSize: 12,
          color: 'var(--color-muted-foreground)',
          marginTop: 0,
          marginBottom: 20,
        }}
      >
        通过 Cloudflare、ngrok 或自定义工具将任意本地端口暴露到公网。 请在{' '}
        <strong>设置 → 隧道</strong> 中配置提供商。
      </p>

      {/* Create form */}
      {showForm && (
        <CreateForm
          defaultProvider={defaultProvider}
          onCreated={() => {
            setShowForm(false)
            handleManualReload()
          }}
          onCancel={() => setShowForm(false)}
        />
      )}

      {/* Error */}
      {error && (
        <div role="alert" style={{ ...card, color: 'var(--color-status-crashed)', fontSize: 13 }}>
          {error}
        </div>
      )}

      {/* Loading skeleton */}
      {loading && !tunnels.length && (
        <div style={{ ...card, color: 'var(--color-muted-foreground)', fontSize: 13 }}>加载中…</div>
      )}

      {/* Active tunnels */}
      {activeTunnels.length > 0 && (
        <>
          <p
            style={{
              fontSize: 11,
              fontWeight: 700,
              letterSpacing: '0.08em',
              color: 'var(--color-muted-foreground)',
              textTransform: 'uppercase',
              marginBottom: 4,
            }}
          >
            活动中
          </p>
          <div style={card}>
            {activeTunnels.map(t => (
              <TunnelRow
                key={t.id}
                tunnel={t}
                onStop={() => handleStop(t.id)}
                onRemove={() => handleRemove(t.id)}
                busy={busyTunnelIds.has(t.id)}
              />
            ))}
          </div>
        </>
      )}

      {/* Inactive tunnels */}
      {inactiveTunnels.length > 0 && (
        <>
          <p
            style={{
              fontSize: 11,
              fontWeight: 700,
              letterSpacing: '0.08em',
              color: 'var(--color-muted-foreground)',
              textTransform: 'uppercase',
              marginBottom: 4,
            }}
          >
            已停止 / 失败
          </p>
          <div style={card}>
            {inactiveTunnels.map(t => (
              <TunnelRow
                key={t.id}
                tunnel={t}
                onStop={() => handleStop(t.id)}
                onRemove={() => handleRemove(t.id)}
                busy={busyTunnelIds.has(t.id)}
              />
            ))}
          </div>
        </>
      )}

      {/* Empty state */}
      {!loading && !error && tunnels.length === 0 && !showForm && (
        <div style={{ ...card, textAlign: 'center', padding: '40px 20px' }}>
          <Globe size={28} style={{ color: 'var(--color-border)', marginBottom: 12 }} />
          <p style={{ fontSize: 14, color: 'var(--color-muted-foreground)', margin: '0 0 16px' }}>
            暂无隧道。点击<strong>新建隧道</strong>以将本地端口暴露到公网。
          </p>
          <button onClick={() => setShowForm(true)} style={btnPrimary}>
            新建隧道
          </button>
        </div>
      )}
    </div>
  )
}
