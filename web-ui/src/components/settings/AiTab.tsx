// @group BusinessLogic > AiTab : AI assistant settings — provider selector, per-provider config, GitHub OAuth

import { useCallback, useEffect, useRef, useState } from 'react'
import { Check, Copy, Github, Loader, LogOut, RefreshCw } from 'lucide-react'
import type { AiModelInfo } from '@/lib/api'
import { api } from '@/lib/api'
import { SettingRow, Toggle } from './shared'
import { card, inputStyle, sectionTitle, selectStyle } from './sharedStyles'
import { useSingleFlightPoll } from '@/hooks/useSingleFlightPoll'

type Provider = 'ollama' | 'copilot' | 'github' | 'claude' | 'openai'

// 'github' kept in type for backwards compat but hidden from the UI selector
const PROVIDER_LABELS: Record<Provider, string> = {
  ollama: 'Ollama（本地）',
  copilot: 'GitHub Copilot',
  github: 'GitHub Models',
  claude: 'Claude (Anthropic)',
  openai: 'OpenAI',
}

const VISIBLE_PROVIDERS: Provider[] = ['ollama', 'copilot', 'claude', 'openai']

export default function AiTab() {
  const [aiEnabled, setAiEnabled] = useState(false)
  const [provider, setProvider] = useState<Provider>('ollama')
  const [aiModel, setAiModel] = useState('llama3.2')
  const [aiSaving, setAiSaving] = useState(false)
  const [aiSaved, setAiSaved] = useState(false)
  const [aiError, setAiError] = useState<string | null>(null)
  const [settingsLoaded, setSettingsLoaded] = useState(false)
  const [settingsLoadError, setSettingsLoadError] = useState<string | null>(null)

  // @group BusinessLogic > AiTab > GitHub : OAuth Device Flow state
  const [aiClientId, setAiClientId] = useState('')
  const [aiClientIdBuiltin, setAiClientIdBuiltin] = useState(false)
  const [authPhase, setAuthPhase] = useState<'idle' | 'in_progress' | 'connected'>('idle')
  const [authUsername, setAuthUsername] = useState('')
  const [deviceUserCode, setDeviceUserCode] = useState('')
  const [deviceUri, setDeviceUri] = useState('')
  const [deviceFlowId, setDeviceFlowId] = useState('')
  const [devicePollToken, setDevicePollToken] = useState('')
  const [pollInterval, setPollInterval] = useState(5)
  const [codeCopied, setCodeCopied] = useState(false)
  const [authError, setAuthError] = useState<string | null>(null)
  const [authStarting, setAuthStarting] = useState(false)
  const authProviderRef = useRef<Provider>('copilot')
  const authDeadlineRef = useRef(0)

  // @group BusinessLogic > AiTab > Claude : Anthropic API key
  const [anthropicKey, setAnthropicKey] = useState('')
  const [anthropicKeyHint, setAnthropicKeyHint] = useState('')
  const [anthropicKeySet, setAnthropicKeySet] = useState(false)

  // @group BusinessLogic > AiTab > OpenAI : OpenAI API key + base URL
  const [openaiKey, setOpenaiKey] = useState('')
  const [openaiKeyHint, setOpenaiKeyHint] = useState('')
  const [openaiKeySet, setOpenaiKeySet] = useState(false)
  const [openaiBaseUrl, setOpenaiBaseUrl] = useState('https://api.openai.com/v1')

  // @group BusinessLogic > AiTab > Ollama : Base URL
  const [ollamaBaseUrl, setOllamaBaseUrl] = useState('http://localhost:11434')

  // @group BusinessLogic > AiTab : Dynamic model list
  const [modelOptions, setModelOptions] = useState<AiModelInfo[]>([])
  const [modelsLoading, setModelsLoading] = useState(false)
  const modelRequestRef = useRef(0)
  const modelAbortRef = useRef<AbortController | null>(null)

  const loadModels = useCallback(async (target: Provider) => {
    const requestId = ++modelRequestRef.current
    modelAbortRef.current?.abort()
    const controller = new AbortController()
    modelAbortRef.current = controller
    setModelsLoading(true)
    setAiError(null)
    try {
      const data = await api.aiGetModels(target, { signal: controller.signal })
      if (requestId !== modelRequestRef.current || controller.signal.aborted) return
      if (data.models.length > 0) {
        setModelOptions(data.models)
        setAiModel(previous =>
          data.models.some(model => model.id === previous) ? previous : data.models[0].id
        )
      } else {
        setAiError('该服务未返回可用模型，请检查连接或账户权限')
      }
    } catch (loadError) {
      if (requestId !== modelRequestRef.current || controller.signal.aborted) return
      setAiError(loadError instanceof Error ? loadError.message : '模型列表加载失败')
    } finally {
      if (requestId === modelRequestRef.current) {
        modelAbortRef.current = null
        setModelsLoading(false)
      }
    }
  }, [])

  useEffect(
    () => () => {
      modelRequestRef.current += 1
      modelAbortRef.current?.abort()
      modelAbortRef.current = null
    },
    []
  )

  useEffect(() => {
    const controller = new AbortController()
    api
      .aiGetSettings({ signal: controller.signal })
      .then(s => {
        if (controller.signal.aborted) return
        setAiEnabled(s.enabled)
        setProvider((s.provider as Provider) || 'ollama')
        setAiModel(s.model)
        setAiClientId('')
        setAiClientIdBuiltin(!!s.client_id_builtin)
        // GitHub
        if (s.github_username && s.github_token_set) {
          setAuthUsername(s.github_username)
          setAuthPhase('connected')
        } else {
          setAuthUsername('')
          setAuthPhase('idle')
        }
        // Claude
        setAnthropicKeySet(s.anthropic_key_set)
        setAnthropicKeyHint(s.anthropic_key_hint ?? '')
        // OpenAI
        setOpenaiKeySet(s.openai_key_set)
        setOpenaiKeyHint(s.openai_key_hint ?? '')
        setOpenaiBaseUrl(s.openai_base_url || 'https://api.openai.com/v1')
        // Ollama
        setOllamaBaseUrl(s.ollama_base_url || 'http://localhost:11434')
        // Load models on startup if credentials are present for the active provider
        const canLoad =
          s.provider === 'ollama' ||
          (s.provider === 'claude' && s.anthropic_key_set) ||
          (s.provider === 'openai' && s.openai_key_set) ||
          ((s.provider === 'copilot' || s.provider === 'github') && s.github_token_set)
        if (canLoad) loadModels(s.provider as Provider)
        setSettingsLoaded(true)
      })
      .catch(loadError => {
        if (controller.signal.aborted) return
        setSettingsLoadError(loadError instanceof Error ? loadError.message : 'AI 设置加载失败')
      })
    return () => controller.abort()
  }, [loadModels])

  // @group BusinessLogic > AiTab > GitHub : Poll during Device Flow
  const pollDeviceAuth = useCallback(
    async (isCurrent: () => boolean, signal: AbortSignal) => {
      if (authPhase !== 'in_progress' || !deviceFlowId || !devicePollToken) return
      if (Date.now() >= authDeadlineRef.current) {
        if (isCurrent()) {
          setAuthPhase('idle')
          setAuthError('GitHub 登录等待已超时，请重新开始。')
        }
        return
      }
      try {
        const status = await api.aiAuthStatus(deviceFlowId, devicePollToken, { signal })
        if (!isCurrent()) return
        if (status.status === 'complete' && status.username) {
          setAuthPhase('connected')
          setDevicePollToken('')
          setAuthUsername(status.username)
          setAuthError(null)
          if (provider === authProviderRef.current) void loadModels(authProviderRef.current)
        } else if (status.status === 'expired') {
          setAuthPhase('idle')
          setDevicePollToken('')
          setAuthError('验证码已过期，请重试。')
        } else if (status.status === 'denied') {
          setAuthPhase('idle')
          setDevicePollToken('')
          setAuthError('GitHub 拒绝了授权。')
        } else if (status.status === 'error') {
          setAuthPhase('idle')
          setDevicePollToken('')
          setAuthError(status.message ?? 'GitHub 返回未知错误。')
        } else if (status.interval) {
          setPollInterval(status.interval)
        }
      } catch (pollError) {
        if (!signal.aborted) {
          setAuthError(pollError instanceof Error ? pollError.message : 'GitHub 授权状态查询失败')
        }
        throw pollError
      }
    },
    [authPhase, deviceFlowId, devicePollToken, loadModels, provider]
  )
  useSingleFlightPoll(pollDeviceAuth, {
    intervalMs: pollInterval * 1_000,
    enabled: authPhase === 'in_progress',
    refreshKey: authPhase,
  })

  async function handleProviderChange(newProvider: Provider) {
    if (authStarting || authPhase === 'in_progress') return
    setProvider(newProvider)
    setModelOptions([])
    setAiModel('')
    await loadModels(newProvider)
  }

  async function saveAiSettings() {
    if (!aiModel.trim()) {
      setAiError('请先成功加载并选择一个模型')
      return
    }
    if (
      aiEnabled &&
      (provider === 'copilot' || provider === 'github') &&
      authPhase !== 'connected'
    ) {
      setAiError('启用 GitHub Copilot 前请先完成 GitHub 登录')
      return
    }
    setAiSaving(true)
    setAiError(null)
    try {
      const body: Parameters<typeof api.aiSaveSettings>[0] = {
        provider,
        enabled: aiEnabled,
        model: aiModel,
      }
      if ((provider === 'github' || provider === 'copilot') && aiClientId.trim()) {
        body.client_id = aiClientId.trim()
      }
      if (provider === 'claude' && anthropicKey) body.anthropic_key = anthropicKey
      if (provider === 'openai' && openaiKey) body.openai_key = openaiKey
      if (provider === 'openai') body.openai_base_url = openaiBaseUrl
      if (provider === 'ollama') body.ollama_base_url = ollamaBaseUrl

      await api.aiSaveSettings(body)
      setAiSaved(true)
      setTimeout(() => setAiSaved(false), 2000)
      // Clear secret inputs after save
      setAnthropicKey('')
      setOpenaiKey('')
      try {
        const s = await api.aiGetSettings()
        setAnthropicKeySet(s.anthropic_key_set)
        setAnthropicKeyHint(s.anthropic_key_hint ?? '')
        setOpenaiKeySet(s.openai_key_set)
        setOpenaiKeyHint(s.openai_key_hint ?? '')
      } catch (refreshError) {
        setAiError(
          refreshError instanceof Error
            ? `设置已保存，但密钥状态刷新失败：${refreshError.message}`
            : '设置已保存，但密钥状态刷新失败，请重新加载页面'
        )
      }
      if (modelOptions.length === 0) void loadModels(provider)
    } catch (error) {
      setAiError(error instanceof Error ? error.message : '保存 AI 设置失败')
    } finally {
      setAiSaving(false)
    }
  }

  async function saveGithubClientId() {
    const clientId = aiClientId.trim()
    if (!/^[A-Za-z0-9._-]{1,128}$/.test(clientId)) {
      setAiError('GitHub Client ID 只能包含 1–128 个字母、数字、点、下划线或连字符')
      return
    }
    setAiSaving(true)
    setAiError(null)
    try {
      await api.aiSaveSettings({ client_id: clientId })
      setAiSaved(true)
      setTimeout(() => setAiSaved(false), 2000)
    } catch (error) {
      setAiError(error instanceof Error ? error.message : '保存 GitHub Client ID 失败')
    } finally {
      setAiSaving(false)
    }
  }

  async function startDeviceFlow() {
    if (authStarting || authPhase === 'in_progress') return
    setAuthStarting(true)
    setAuthError(null)
    authProviderRef.current = provider
    try {
      const data = await api.aiAuthStart()
      setDeviceFlowId(data.flow_id)
      setDevicePollToken(data.poll_token)
      setDeviceUserCode(data.user_code)
      setDeviceUri(data.verification_uri)
      setPollInterval(data.interval)
      authDeadlineRef.current = Date.now() + Math.min(data.expires_in * 1_000, 15 * 60_000)
      setAuthPhase('in_progress')
    } catch (e: unknown) {
      setAuthError(e instanceof Error ? e.message : '启动 GitHub 登录失败。')
    } finally {
      setAuthStarting(false)
    }
  }

  function cancelDeviceFlow() {
    setAuthPhase('idle')
    setDeviceUserCode('')
    setDeviceUri('')
    setDeviceFlowId('')
    setDevicePollToken('')
    authDeadlineRef.current = 0
    setAuthError(null)
  }

  async function disconnect() {
    setAuthError(null)
    try {
      await api.aiAuthLogout()
      modelRequestRef.current += 1
      modelAbortRef.current?.abort()
      modelAbortRef.current = null
      setModelsLoading(false)
      setAuthPhase('idle')
      setAuthUsername('')
      setModelOptions([])
      setAuthError(null)
    } catch (error) {
      setAuthError(error instanceof Error ? error.message : '断开 GitHub 账户失败')
    }
  }

  async function clearProviderSecret(kind: 'anthropic' | 'openai') {
    setAiSaving(true)
    setAiError(null)
    try {
      await api.aiSaveSettings(
        kind === 'anthropic'
          ? { clear_anthropic_key: true, enabled: false }
          : { clear_openai_key: true, enabled: false }
      )
      setAiEnabled(false)
      if (kind === 'anthropic') {
        setAnthropicKey('')
        setAnthropicKeySet(false)
        setAnthropicKeyHint('')
      } else {
        setOpenaiKey('')
        setOpenaiKeySet(false)
        setOpenaiKeyHint('')
      }
      setModelOptions([])
      setAiModel('')
    } catch (clearError) {
      setAiError(clearError instanceof Error ? clearError.message : '清除 AI 密钥失败')
    } finally {
      setAiSaving(false)
    }
  }

  function copyDeviceCode() {
    navigator.clipboard
      .writeText(deviceUserCode)
      .then(() => {
        setCodeCopied(true)
        setTimeout(() => setCodeCopied(false), 2000)
      })
      .catch(() => setAuthError('复制验证码失败，请手动选择并复制。'))
  }

  const providerNeedsLogin =
    aiEnabled && (provider === 'copilot' || provider === 'github') && authPhase !== 'connected'
  const saveDisabled = aiSaving || modelsLoading || !aiModel.trim() || providerNeedsLogin

  const SaveButton = ({ label = '保存' }: { label?: string }) => (
    <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'flex-end', gap: 4 }}>
      <button
        onClick={saveAiSettings}
        disabled={saveDisabled}
        style={{
          padding: '5px 14px',
          fontSize: 12,
          fontWeight: 500,
          background: aiSaved ? 'var(--color-status-running)' : 'var(--color-primary)',
          color: '#fff',
          border: 'none',
          borderRadius: 5,
          cursor: saveDisabled ? 'not-allowed' : 'pointer',
          opacity: saveDisabled ? 0.6 : 1,
          transition: 'background 0.2s',
        }}
      >
        {aiSaved ? '已保存！' : aiSaving ? '保存中…' : label}
      </button>
      {aiError && (
        <span
          role="alert"
          style={{ maxWidth: 240, fontSize: 11, color: 'var(--color-destructive)' }}
        >
          {aiError}
        </span>
      )}
    </div>
  )

  if (settingsLoadError) {
    return (
      <div role="alert" style={{ color: 'var(--color-destructive)', padding: 16 }}>
        AI 设置加载失败，已禁止保存默认值：{settingsLoadError}
      </div>
    )
  }
  if (!settingsLoaded) {
    return <div style={{ padding: 16, color: 'var(--color-muted-foreground)' }}>加载中…</div>
  }

  return (
    <>
      <p style={sectionTitle}>AI 助手</p>
      <div style={card}>
        {/* 启用开关 */}
        <SettingRow
          label="启用 AI 助手"
          description="在侧边栏显示 AI 面板按钮。"
          control={<Toggle checked={aiEnabled} onChange={setAiEnabled} />}
        />

        {/* 服务提供商选择 */}
        <SettingRow
          label="服务提供商"
          description="选择用于聊天回复的 AI 服务。"
          control={
            <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
              <select
                aria-label="AI 服务提供商"
                value={provider}
                onChange={e => {
                  void handleProviderChange(e.target.value as Provider)
                }}
                disabled={modelsLoading || authStarting || authPhase === 'in_progress'}
                style={{ ...selectStyle, minWidth: 180 }}
              >
                {VISIBLE_PROVIDERS.map(p => (
                  <option key={p} value={p}>
                    {PROVIDER_LABELS[p]}
                  </option>
                ))}
              </select>
              <SaveButton label="保存" />
            </div>
          }
        />

        {/* ── GitHub Copilot 区域 ── */}
        {provider === 'copilot' && (
          <SettingRow
            label="GitHub Copilot"
            description={
              authPhase === 'connected' ? (
                <>
                  通过 <strong>@{authUsername}</strong> 使用当前 Copilot 订阅。无需其他配置。
                </>
              ) : (
                <>
                  需要有效的{' '}
                  <a
                    href="https://github.com/features/copilot"
                    target="_blank"
                    rel="noreferrer"
                    style={{ color: 'var(--color-primary)', textDecoration: 'none' }}
                  >
                    GitHub Copilot
                  </a>{' '}
                  订阅。请先使用 GitHub 提供商登录以关联账户。
                </>
              )
            }
            control={
              authPhase === 'connected' ? (
                <span
                  style={{
                    fontSize: 11,
                    fontWeight: 600,
                    padding: '3px 10px',
                    borderRadius: 4,
                    background: 'color-mix(in srgb, var(--color-status-running) 18%, transparent)',
                    color: 'var(--color-status-running)',
                  }}
                >
                  ✓ 就绪
                </span>
              ) : (
                <span
                  style={{
                    fontSize: 11,
                    fontWeight: 600,
                    padding: '3px 10px',
                    borderRadius: 4,
                    background: 'color-mix(in srgb, var(--color-destructive) 15%, transparent)',
                    color: 'var(--color-destructive)',
                  }}
                >
                  未连接
                </span>
              )
            }
          />
        )}

        {/* ── GitHub Models 区域 ── */}
        {(provider === 'github' || provider === 'copilot') && (
          <>
            <SettingRow
              label="GitHub OAuth 应用 Client ID"
              description={
                aiClientIdBuiltin ? (
                  'Client ID 已内置于此程序，无需手动配置。'
                ) : (
                  <>
                    请在{' '}
                    <a
                      href="https://github.com/settings/developers"
                      target="_blank"
                      rel="noreferrer"
                      style={{ color: 'var(--color-primary)', textDecoration: 'none' }}
                    >
                      github.com/settings/developers
                    </a>{' '}
                    创建 OAuth 应用并启用“设备流程”，然后将 Client ID 粘贴到此处（无需密钥）。
                  </>
                )
              }
              control={
                aiClientIdBuiltin ? (
                  <span
                    style={{
                      fontSize: 11,
                      fontWeight: 600,
                      padding: '3px 10px',
                      borderRadius: 4,
                      background:
                        'color-mix(in srgb, var(--color-status-running) 18%, transparent)',
                      color: 'var(--color-status-running)',
                    }}
                  >
                    ✓ 已内置
                  </span>
                ) : (
                  <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                    <input
                      aria-label="GitHub OAuth 应用 Client ID"
                      type="text"
                      value={aiClientId}
                      onChange={e => setAiClientId(e.target.value)}
                      maxLength={128}
                      pattern="[A-Za-z0-9._-]+"
                      placeholder="Oauth_…"
                      style={{
                        ...inputStyle,
                        width: 180,
                        fontSize: 12,
                        padding: '5px 10px',
                        fontFamily: 'monospace',
                      }}
                      spellCheck={false}
                      autoComplete="off"
                    />
                    <button
                      type="button"
                      onClick={saveGithubClientId}
                      disabled={aiSaving || !aiClientId.trim()}
                      style={{
                        padding: '5px 14px',
                        fontSize: 12,
                        fontWeight: 500,
                        background: aiSaved
                          ? 'var(--color-status-running)'
                          : 'var(--color-primary)',
                        color: '#fff',
                        border: 'none',
                        borderRadius: 5,
                        cursor: aiSaving || !aiClientId.trim() ? 'not-allowed' : 'pointer',
                        opacity: aiSaving || !aiClientId.trim() ? 0.6 : 1,
                      }}
                    >
                      {aiSaved ? '已保存！' : aiSaving ? '保存中…' : '保存'}
                    </button>
                  </div>
                )
              }
            />

            <SettingRow
              label="GitHub 账户"
              description="登录后，RunDock 可通过 GitHub OAuth 自动获取访问令牌。"
              control={
                <div
                  style={{
                    display: 'flex',
                    flexDirection: 'column',
                    alignItems: 'flex-end',
                    gap: 6,
                  }}
                >
                  {authPhase === 'idle' && (
                    <button
                      onClick={startDeviceFlow}
                      disabled={authStarting}
                      style={{
                        display: 'inline-flex',
                        alignItems: 'center',
                        gap: 6,
                        padding: '6px 14px',
                        fontSize: 12,
                        fontWeight: 500,
                        background: 'var(--color-foreground)',
                        color: 'var(--color-background)',
                        border: 'none',
                        borderRadius: 5,
                        cursor: authStarting ? 'wait' : 'pointer',
                        opacity: authStarting ? 0.6 : 1,
                      }}
                    >
                      <Github size={13} /> {authStarting ? '正在启动…' : '使用 GitHub 登录'}
                    </button>
                  )}
                  {authPhase === 'in_progress' && (
                    <div
                      style={{
                        display: 'flex',
                        flexDirection: 'column',
                        alignItems: 'flex-end',
                        gap: 6,
                        padding: '10px 12px',
                        background: 'var(--color-accent)',
                        borderRadius: 8,
                        border: '1px solid var(--color-border)',
                        minWidth: 230,
                      }}
                    >
                      <div
                        style={{
                          fontSize: 11,
                          color: 'var(--color-muted-foreground)',
                          alignSelf: 'flex-start',
                        }}
                      >
                        请在以下地址输入此验证码：
                      </div>
                      <div style={{ alignSelf: 'flex-start' }}>
                        <a
                          href={deviceUri || 'https://github.com/login/device'}
                          target="_blank"
                          rel="noreferrer"
                          style={{
                            fontSize: 11,
                            color: 'var(--color-primary)',
                            textDecoration: 'none',
                          }}
                        >
                          {deviceUri || 'github.com/login/device'} ↗
                        </a>
                      </div>
                      <div
                        style={{
                          display: 'flex',
                          alignItems: 'center',
                          gap: 8,
                          alignSelf: 'flex-start',
                        }}
                      >
                        <code
                          style={{
                            fontSize: 22,
                            fontWeight: 700,
                            fontFamily: 'monospace',
                            letterSpacing: '0.12em',
                            color: 'var(--color-foreground)',
                          }}
                        >
                          {deviceUserCode}
                        </code>
                        <button
                          onClick={copyDeviceCode}
                          title="复制验证码"
                          style={{
                            background: 'transparent',
                            border: 'none',
                            cursor: 'pointer',
                            color: codeCopied
                              ? 'var(--color-status-running)'
                              : 'var(--color-muted-foreground)',
                            display: 'flex',
                            alignItems: 'center',
                          }}
                        >
                          {codeCopied ? <Check size={14} /> : <Copy size={14} />}
                        </button>
                      </div>
                      <div
                        style={{
                          display: 'flex',
                          alignItems: 'center',
                          gap: 8,
                          alignSelf: 'flex-start',
                        }}
                      >
                        <Loader
                          size={12}
                          style={{
                            animation: 'spin 1s linear infinite',
                            color: 'var(--color-muted-foreground)',
                          }}
                        />
                        <span style={{ fontSize: 11, color: 'var(--color-muted-foreground)' }}>
                          等待授权…
                        </span>
                      </div>
                      <button
                        onClick={cancelDeviceFlow}
                        style={{
                          alignSelf: 'flex-start',
                          padding: '4px 10px',
                          fontSize: 11,
                          background: 'transparent',
                          border: '1px solid var(--color-border)',
                          borderRadius: 4,
                          cursor: 'pointer',
                          color: 'var(--color-muted-foreground)',
                        }}
                      >
                        取消
                      </button>
                    </div>
                  )}
                  {authPhase === 'connected' && (
                    <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
                      <span
                        style={{
                          fontSize: 12,
                          color: 'var(--color-status-running)',
                          fontWeight: 500,
                        }}
                      >
                        ✓ 已连接为 @{authUsername}
                      </span>
                      <button
                        onClick={disconnect}
                        title="断开 GitHub 账户"
                        style={{
                          display: 'inline-flex',
                          alignItems: 'center',
                          gap: 4,
                          padding: '4px 10px',
                          fontSize: 11,
                          background: 'transparent',
                          border: '1px solid var(--color-border)',
                          borderRadius: 4,
                          cursor: 'pointer',
                          color: 'var(--color-muted-foreground)',
                        }}
                      >
                        <LogOut size={11} /> 断开连接
                      </button>
                    </div>
                  )}
                  {authError && (
                    <div
                      role="alert"
                      aria-live="polite"
                      style={{
                        fontSize: 11,
                        color: 'var(--color-destructive)',
                        maxWidth: 240,
                        textAlign: 'right',
                      }}
                    >
                      {authError}
                    </div>
                  )}
                </div>
              }
            />
          </>
        )}

        {/* ── Claude 区域 ── */}
        {provider === 'claude' && (
          <SettingRow
            label="Anthropic API 密钥"
            description={
              <>
                可在此处获取密钥：{' '}
                <a
                  href="https://console.anthropic.com/"
                  target="_blank"
                  rel="noreferrer"
                  style={{ color: 'var(--color-primary)', textDecoration: 'none' }}
                >
                  console.anthropic.com
                </a>
              </>
            }
            control={
              <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                <input
                  aria-label="Anthropic API 密钥"
                  type="password"
                  value={anthropicKey}
                  onChange={e => setAnthropicKey(e.target.value)}
                  placeholder={anthropicKeySet ? anthropicKeyHint || '••••••••' : 'sk-ant-…'}
                  style={{
                    ...inputStyle,
                    width: 200,
                    fontSize: 12,
                    padding: '5px 10px',
                    fontFamily: 'monospace',
                  }}
                  spellCheck={false}
                  autoComplete="off"
                />
                {anthropicKeySet && (
                  <button
                    type="button"
                    disabled={aiSaving}
                    onClick={() => void clearProviderSecret('anthropic')}
                    style={{ ...inputStyle, width: 'auto', cursor: 'pointer' }}
                  >
                    清除
                  </button>
                )}
                <SaveButton />
              </div>
            }
          />
        )}

        {/* ── OpenAI 区域 ── */}
        {provider === 'openai' && (
          <>
            <SettingRow
              label="OpenAI API 密钥"
              description="你的 OpenAI 私密密钥（sk-…）。留空以保留现有密钥。"
              control={
                <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                  <input
                    aria-label="OpenAI API 密钥"
                    type="password"
                    value={openaiKey}
                    onChange={e => setOpenaiKey(e.target.value)}
                    placeholder={openaiKeySet ? openaiKeyHint || '••••••••' : 'sk-…'}
                    style={{
                      ...inputStyle,
                      width: 200,
                      fontSize: 12,
                      padding: '5px 10px',
                      fontFamily: 'monospace',
                    }}
                    spellCheck={false}
                    autoComplete="off"
                  />
                  {openaiKeySet && (
                    <button
                      type="button"
                      disabled={aiSaving}
                      onClick={() => void clearProviderSecret('openai')}
                      style={{ ...inputStyle, width: 'auto', cursor: 'pointer' }}
                    >
                      清除
                    </button>
                  )}
                </div>
              }
            />
            <SettingRow
              label="API 基础 URL"
              description="用于覆盖 OpenAI 兼容端点（例如 Azure、LM Studio、Groq）。"
              control={
                <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                  <input
                    aria-label="OpenAI API 基础 URL"
                    type="text"
                    value={openaiBaseUrl}
                    onChange={e => setOpenaiBaseUrl(e.target.value)}
                    placeholder="https://api.openai.com/v1"
                    style={{
                      ...inputStyle,
                      width: 240,
                      fontSize: 12,
                      padding: '5px 10px',
                      fontFamily: 'monospace',
                    }}
                    spellCheck={false}
                    autoComplete="off"
                  />
                  <SaveButton />
                </div>
              }
            />
          </>
        )}

        {/* ── Ollama 区域 ── */}
        {provider === 'ollama' && (
          <SettingRow
            label="Ollama 基础 URL"
            description="本地 Ollama 实例的 URL。"
            control={
              <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                <input
                  aria-label="Ollama 基础 URL"
                  type="text"
                  value={ollamaBaseUrl}
                  onChange={e => setOllamaBaseUrl(e.target.value)}
                  placeholder="http://localhost:11434"
                  style={{
                    ...inputStyle,
                    width: 220,
                    fontSize: 12,
                    padding: '5px 10px',
                    fontFamily: 'monospace',
                  }}
                  spellCheck={false}
                  autoComplete="off"
                />
                <SaveButton />
              </div>
            }
          />
        )}

        {/* ── 模型选择（所有提供商） ── */}
        <SettingRow
          label="模型"
          description={`用于聊天回复的 ${PROVIDER_LABELS[provider]} 模型。`}
          isLast
          control={
            <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
              <select
                aria-label="AI 模型"
                value={aiModel}
                onChange={e => setAiModel(e.target.value)}
                disabled={modelsLoading || modelOptions.length === 0}
                style={{ ...selectStyle, minWidth: 200 }}
              >
                {modelOptions.length > 0 ? (
                  modelOptions.map(m => (
                    <option key={m.id} value={m.id}>
                      {m.label}
                      {m.publisher ? ` (${m.publisher})` : ''}
                    </option>
                  ))
                ) : (
                  <option value={aiModel}>{aiModel || '暂无可用模型'}</option>
                )}
              </select>
              <button
                onClick={() => void loadModels(provider)}
                disabled={modelsLoading}
                title="刷新模型列表"
                style={{
                  display: 'flex',
                  alignItems: 'center',
                  justifyContent: 'center',
                  width: 28,
                  height: 28,
                  borderRadius: 5,
                  background: 'transparent',
                  border: '1px solid var(--color-border)',
                  cursor: 'pointer',
                  color: 'var(--color-muted-foreground)',
                }}
              >
                <RefreshCw
                  size={12}
                  style={modelsLoading ? { animation: 'spin 1s linear infinite' } : {}}
                />
              </button>
              <SaveButton />
            </div>
          }
        />
      </div>
    </>
  )
}
