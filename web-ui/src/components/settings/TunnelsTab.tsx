// @group BusinessLogic > TunnelsTab : Tunnel provider settings — Cloudflare, ngrok, custom binary

import { useEffect, useRef, useState } from 'react'
import { CheckCircle, XCircle } from 'lucide-react'
import type { TunnelProvider, TunnelSettings } from '@/types'
import { secretInputPlaceholder, secretInputValue } from '@/lib/secrets'
import { api } from '@/lib/api'
import { parseInstallStreamEvent } from '@/lib/tunnels'
import { SettingRow } from './shared'
import { card, inputStyle, sectionTitle } from './sharedStyles'

const MAX_INSTALL_OUTPUT_LINES = 500
const INSTALL_STREAM_TIMEOUT_MS = 2 * 60_000

export default function TunnelsTab() {
  const [tnSettings, setTnSettings] = useState<TunnelSettings>({
    provider: 'cloudflare',
    cloudflare: { token: null },
    ngrok: { auth_token: null },
    custom: { binary_path: '', args_template: '' },
  })
  const [tnSaving, setTnSaving] = useState(false)
  const [tnSaved, setTnSaved] = useState(false)
  const [tnError, setTnError] = useState<string | null>(null)
  const [settingsLoaded, setSettingsLoaded] = useState(false)
  const [settingsLoadError, setSettingsLoadError] = useState<string | null>(null)
  const [tnTestResult, setTnTestResult] = useState<
    Record<TunnelProvider, { ok: boolean; message: string } | null>
  >({
    cloudflare: null,
    ngrok: null,
    custom: null,
  })
  const [tnTesting, setTnTesting] = useState<TunnelProvider | null>(null)
  const [tnInstalling, setTnInstalling] = useState<TunnelProvider | null>(null)
  const [tnInstallLines, setTnInstallLines] = useState<Record<TunnelProvider, string[]>>({
    cloudflare: [],
    ngrok: [],
    custom: [],
  })
  const [tnInstallDone, setTnInstallDone] = useState<Record<TunnelProvider, boolean | null>>({
    cloudflare: null,
    ngrok: null,
    custom: null,
  })
  const installEsRef = useRef<{
    es: EventSource
    provider: TunnelProvider
    timeoutId: number
  } | null>(null)
  const installRequestRef = useRef(0)
  const installTicketAbortRef = useRef<AbortController | null>(null)
  const terminalRefs = useRef<Record<TunnelProvider, HTMLPreElement | null>>({
    cloudflare: null,
    ngrok: null,
    custom: null,
  })

  useEffect(() => {
    api
      .getTunnelSettings()
      .then(s => {
        setTnSettings(s)
        setSettingsLoaded(true)
      })
      .catch(error => {
        setSettingsLoadError(error instanceof Error ? error.message : '读取隧道设置失败')
      })
  }, [])

  useEffect(
    () => () => {
      installRequestRef.current += 1
      installTicketAbortRef.current?.abort()
      if (installEsRef.current) {
        window.clearTimeout(installEsRef.current.timeoutId)
        installEsRef.current.es.close()
      }
      installEsRef.current = null
    },
    []
  )

  const btnStyle = (primary: boolean, active: boolean): React.CSSProperties => ({
    padding: '4px 12px',
    fontSize: 12,
    background: primary ? 'var(--color-primary)' : 'transparent',
    border: primary ? 'none' : '1px solid var(--color-border)',
    borderRadius: 5,
    cursor: 'pointer',
    color: primary ? '#fff' : 'var(--color-foreground)',
    opacity: active ? 0.6 : 1,
  })

  async function startInstallStream(provider: TunnelProvider) {
    const requestId = ++installRequestRef.current
    installTicketAbortRef.current?.abort()
    const ticketAbort = new AbortController()
    installTicketAbortRef.current = ticketAbort
    // Close any existing stream
    if (installEsRef.current) {
      window.clearTimeout(installEsRef.current.timeoutId)
      installEsRef.current.es.close()
      installEsRef.current = null
    }
    setTnInstalling(provider)
    setTnInstallLines(prev => ({ ...prev, [provider]: [] }))
    setTnInstallDone(prev => ({ ...prev, [provider]: null }))

    let es: EventSource
    try {
      es = await api.streamInstallProvider(provider, { signal: ticketAbort.signal })
    } catch (error) {
      if (requestId !== installRequestRef.current) return
      setTnInstalling(null)
      setTnInstallDone(prev => ({ ...prev, [provider]: false }))
      setTnInstallLines(prev => ({
        ...prev,
        [provider]: [error instanceof Error ? error.message : '无法创建安全流凭据'],
      }))
      return
    }
    if (requestId !== installRequestRef.current) {
      es.close()
      return
    }
    if (installTicketAbortRef.current === ticketAbort) installTicketAbortRef.current = null
    const timeoutId = window.setTimeout(() => {
      if (requestId !== installRequestRef.current) return
      es.close()
      installEsRef.current = null
      setTnInstalling(null)
      setTnInstallDone(prev => ({ ...prev, [provider]: false }))
      setTnInstallLines(prev => ({
        ...prev,
        [provider]: [...prev[provider], '安装等待已超时，连接已关闭。'].slice(
          -MAX_INSTALL_OUTPUT_LINES
        ),
      }))
    }, INSTALL_STREAM_TIMEOUT_MS)
    installEsRef.current = { es, provider, timeoutId }

    es.onmessage = e => {
      if (requestId !== installRequestRef.current) return
      const data = parseInstallStreamEvent(e.data)
      if (data) {
        if ('done' in data) {
          setTnInstallDone(prev => ({ ...prev, [provider]: data.ok }))
          setTnInstalling(null)
          window.clearTimeout(timeoutId)
          es.close()
          if (installEsRef.current?.es === es) installEsRef.current = null
          // Scroll terminal to bottom
          const el = terminalRefs.current[provider]
          if (el) el.scrollTop = el.scrollHeight
        } else {
          setTnInstallLines(prev => {
            const next = [...prev[provider], data.line].slice(-MAX_INSTALL_OUTPUT_LINES)
            // Scroll terminal to bottom after update
            requestAnimationFrame(() => {
              const el = terminalRefs.current[provider]
              if (el) el.scrollTop = el.scrollHeight
            })
            return { ...prev, [provider]: next }
          })
        }
      } else {
        setTnInstallLines(prev => ({
          ...prev,
          [provider]: [...prev[provider], '收到格式无效的安装输出。'].slice(
            -MAX_INSTALL_OUTPUT_LINES
          ),
        }))
      }
    }

    es.onerror = () => {
      if (requestId !== installRequestRef.current) return
      setTnInstallLines(prev => ({
        ...prev,
        [provider]: [...prev[provider], '连接错误，安装可能仍在运行。'].slice(
          -MAX_INSTALL_OUTPUT_LINES
        ),
      }))
      setTnInstallDone(prev => ({ ...prev, [provider]: false }))
      setTnInstalling(null)
      window.clearTimeout(timeoutId)
      es.close()
      if (installEsRef.current?.es === es) installEsRef.current = null
    }
  }

  function cancelInstall(provider: TunnelProvider) {
    if (tnInstalling !== provider) return
    installRequestRef.current += 1
    installTicketAbortRef.current?.abort()
    installTicketAbortRef.current = null
    const active = installEsRef.current
    if (active?.provider === provider) {
      window.clearTimeout(active.timeoutId)
      active.es.close()
    }
    installEsRef.current = null
    setTnInstalling(null)
    setTnInstallDone(prev => ({ ...prev, [provider]: false }))
    setTnInstallLines(prev => ({
      ...prev,
      [provider]: [...prev[provider], '已取消等待安装输出。'].slice(-MAX_INSTALL_OUTPUT_LINES),
    }))
  }

  function ProviderTestInstall({
    provider,
    hasInstall,
  }: {
    provider: TunnelProvider
    hasInstall: boolean
  }) {
    const lines = tnInstallLines[provider]
    const done = tnInstallDone[provider]
    const hasOutput = lines.length > 0

    return (
      <>
        <div style={{ display: 'flex', gap: 6 }}>
          <button
            onClick={async () => {
              setTnTesting(provider)
              const r = await api.testTunnelProvider(provider).catch(e => ({
                ok: false,
                message: e instanceof Error ? e.message : '隧道提供商测试失败',
              }))
              setTnTestResult(prev => ({ ...prev, [provider]: r }))
              setTnTesting(null)
            }}
            disabled={tnTesting !== null || tnInstalling !== null}
            style={btnStyle(false, tnTesting === provider)}
          >
            {tnTesting === provider ? '测试中…' : '测试'}
          </button>
          {hasInstall &&
            (tnInstalling === provider ? (
              <button onClick={() => cancelInstall(provider)} style={btnStyle(false, false)}>
                取消等待
              </button>
            ) : (
              <button
                onClick={() => startInstallStream(provider)}
                disabled={tnInstalling !== null || tnTesting !== null}
                style={btnStyle(true, false)}
              >
                安装
              </button>
            ))}
        </div>

        {tnTestResult[provider] && (
          <div
            role={tnTestResult[provider]!.ok ? 'status' : 'alert'}
            style={{
              display: 'flex',
              alignItems: 'center',
              gap: 6,
              marginTop: 6,
              fontSize: 12,
              color: tnTestResult[provider]!.ok
                ? 'var(--color-status-running)'
                : 'var(--color-status-crashed)',
            }}
          >
            {tnTestResult[provider]!.ok ? <CheckCircle size={13} /> : <XCircle size={13} />}
            {tnTestResult[provider]!.message}
          </div>
        )}

        {(hasOutput || tnInstalling === provider) && (
          <div style={{ marginTop: 10 }}>
            {/* Terminal header */}
            <div
              style={{
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'space-between',
                background: '#1a1a1a',
                borderRadius: '6px 6px 0 0',
                padding: '5px 10px',
                borderBottom: '1px solid #333',
              }}
            >
              <div style={{ display: 'flex', gap: 6 }}>
                <span
                  style={{
                    width: 10,
                    height: 10,
                    borderRadius: '50%',
                    background: '#ff5f57',
                    display: 'inline-block',
                  }}
                />
                <span
                  style={{
                    width: 10,
                    height: 10,
                    borderRadius: '50%',
                    background: '#ffbd2e',
                    display: 'inline-block',
                  }}
                />
                <span
                  style={{
                    width: 10,
                    height: 10,
                    borderRadius: '50%',
                    background: '#28c840',
                    display: 'inline-block',
                  }}
                />
              </div>
              <span style={{ fontSize: 10, color: '#666', fontFamily: 'monospace' }}>
                {tnInstalling === provider ? '安装中…' : done === true ? '完成' : '失败'}
              </span>
            </div>
            {/* Terminal body */}
            <pre
              ref={el => {
                terminalRefs.current[provider] = el
              }}
              style={{
                fontSize: 11,
                fontFamily: 'monospace',
                background: '#0d0d0d',
                border: '1px solid #333',
                borderTop: 'none',
                borderRadius: '0 0 6px 6px',
                padding: '10px 12px',
                margin: 0,
                whiteSpace: 'pre-wrap',
                wordBreak: 'break-all',
                maxHeight: 220,
                overflow: 'auto',
                color: '#d4d4d4',
                lineHeight: 1.5,
              }}
            >
              {lines.length === 0 ? (
                <span style={{ color: '#555' }}>等待输出…</span>
              ) : (
                lines.map((line, i) => <div key={i}>{line}</div>)
              )}
              {tnInstalling === provider && (
                <span
                  style={{
                    display: 'inline-block',
                    width: 8,
                    height: 14,
                    background: '#d4d4d4',
                    verticalAlign: 'text-bottom',
                    animation: 'blink 1s step-end infinite',
                  }}
                />
              )}
            </pre>
            {/* Status line below terminal */}
            {done !== null && (
              <div
                style={{
                  display: 'flex',
                  alignItems: 'center',
                  gap: 6,
                  marginTop: 6,
                  fontSize: 12,
                  color: done ? 'var(--color-status-running)' : 'var(--color-status-crashed)',
                }}
              >
                {done ? <CheckCircle size={13} /> : <XCircle size={13} />}
                {done ? '安装成功' : '安装失败'}
              </div>
            )}
          </div>
        )}
        <style>{`@keyframes blink { 0%,100%{opacity:1} 50%{opacity:0} }`}</style>
      </>
    )
  }

  if (settingsLoadError) {
    return (
      <div role="alert" style={{ color: 'var(--color-destructive)', padding: 16 }}>
        隧道设置加载失败，已禁止保存默认值：{settingsLoadError}
      </div>
    )
  }
  if (!settingsLoaded) {
    return <div style={{ padding: 16, color: 'var(--color-muted-foreground)' }}>加载中…</div>
  }

  return (
    <>
      <p style={sectionTitle}>隧道提供商</p>
      <div style={card}>
        <SettingRow
          label="默认提供商"
          description="从“隧道”页面或“端口查找”创建隧道时使用的工具。"
          isLast
          control={
            <select
              aria-label="默认隧道提供商"
              value={tnSettings.provider}
              onChange={e =>
                setTnSettings(prev => ({ ...prev, provider: e.target.value as TunnelProvider }))
              }
              style={{ ...inputStyle, width: 160, padding: '4px 8px' }}
            >
              <option value="cloudflare">Cloudflare</option>
              <option value="ngrok">ngrok</option>
              <option value="custom">自定义</option>
            </select>
          }
        />
      </div>

      <p style={sectionTitle}>Cloudflare</p>
      <div style={card}>
        <SettingRow
          label="快速隧道"
          description={
            <>
              无需账户。使用 <code>cloudflared tunnel --url</code>，每次都会生成随机的{' '}
              <code>trycloudflare.com</code> URL。
            </>
          }
          control={null}
        />
        <SettingRow
          label="命名隧道令牌"
          description="可选。粘贴 Cloudflare 隧道令牌，以便在自己的域名上使用稳定主机名。快速隧道可留空。"
          control={
            <input
              aria-label="Cloudflare 命名隧道令牌"
              type="password"
              value={secretInputValue(tnSettings.cloudflare.token)}
              placeholder={secretInputPlaceholder(tnSettings.cloudflare.token, 'eyJhIjoiL…')}
              onChange={e =>
                setTnSettings(prev => ({
                  ...prev,
                  cloudflare: { ...prev.cloudflare, token: e.target.value || null },
                }))
              }
              style={{ ...inputStyle, width: 240 }}
            />
          }
        />
        <SettingRow
          label="二进制文件"
          description={
            <>
              可通过 <code>winget</code>（Windows）或 <code>brew</code>（macOS）自动安装，也可从{' '}
              <code>developers.cloudflare.com/cloudflared</code> 下载。
            </>
          }
          isLast
          control={<ProviderTestInstall provider="cloudflare" hasInstall />}
        />
      </div>

      <p style={sectionTitle}>ngrok</p>
      <div style={card}>
        <SettingRow
          label="身份验证令牌"
          description={
            <>
              免费 URL 可选。可在 <code>dashboard.ngrok.com/get-started/your-authtoken</code> 获取。
            </>
          }
          control={
            <input
              aria-label="ngrok 身份验证令牌"
              type="password"
              value={secretInputValue(tnSettings.ngrok.auth_token)}
              placeholder={secretInputPlaceholder(tnSettings.ngrok.auth_token, '2abc…')}
              onChange={e =>
                setTnSettings(prev => ({
                  ...prev,
                  ngrok: { ...prev.ngrok, auth_token: e.target.value || null },
                }))
              }
              style={{ ...inputStyle, width: 240 }}
            />
          }
        />
        <SettingRow
          label="二进制文件"
          description={
            <>
              安装：<code>winget install ngrok.ngrok</code>，或从 <code>ngrok.com/download</code>{' '}
              下载。
            </>
          }
          isLast
          control={<ProviderTestInstall provider="ngrok" hasInstall />}
        />
      </div>

      <p style={sectionTitle}>自定义提供商</p>
      <div style={card}>
        <SettingRow
          label="二进制路径"
          description='隧道二进制文件的完整路径（例如 "bore"、"lt" 或 "C:\\tools\\mytunnel.exe"）。'
          control={
            <input
              aria-label="自定义隧道二进制路径"
              type="text"
              placeholder="bore"
              value={tnSettings.custom.binary_path}
              onChange={e =>
                setTnSettings(prev => ({
                  ...prev,
                  custom: { ...prev.custom, binary_path: e.target.value },
                }))
              }
              style={{ ...inputStyle, width: 200 }}
            />
          }
        />
        <SettingRow
          label="参数模板"
          description='命令参数，其中 {port} 作为端口占位符（例如 "local {port}"）。二进制文件调用方式为：binary [args]。'
          control={
            <input
              aria-label="自定义隧道参数模板"
              type="text"
              placeholder="local {port}"
              value={tnSettings.custom.args_template}
              onChange={e =>
                setTnSettings(prev => ({
                  ...prev,
                  custom: { ...prev.custom, args_template: e.target.value },
                }))
              }
              style={{ ...inputStyle, width: 200 }}
            />
          }
        />
        <SettingRow
          label="二进制文件"
          description="自定义二进制文件必须在 stdout 或 stderr 输出中的某处打印 https:// URL。"
          isLast
          control={<ProviderTestInstall provider="custom" hasInstall={false} />}
        />
      </div>

      <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
        <button
          onClick={async () => {
            setTnSaving(true)
            setTnSaved(false)
            setTnError(null)
            try {
              await api.updateTunnelSettings(tnSettings)
              setTnSaved(true)
              setTimeout(() => setTnSaved(false), 2500)
            } catch (e: unknown) {
              setTnError(e instanceof Error ? e.message : '保存失败')
            } finally {
              setTnSaving(false)
            }
          }}
          disabled={tnSaving}
          style={{
            padding: '7px 18px',
            fontSize: 13,
            fontWeight: 500,
            background: tnSaved ? 'var(--color-status-running)' : 'var(--color-primary)',
            color: '#fff',
            border: 'none',
            borderRadius: 6,
            cursor: 'pointer',
            opacity: tnSaving ? 0.6 : 1,
            transition: 'background 0.2s',
          }}
        >
          {tnSaved ? '已保存！' : tnSaving ? '保存中…' : '保存'}
        </button>
        {tnError && (
          <span style={{ fontSize: 12, color: 'var(--color-status-crashed)' }}>{tnError}</span>
        )}
      </div>
    </>
  )
}
