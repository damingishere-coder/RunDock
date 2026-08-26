// @group BusinessLogic : Start new process form

import { useRef, useState } from 'react'
import { FolderOpen } from 'lucide-react'
import { api } from '@/lib/api'
import { NamespaceInput } from '@/components/NamespaceInput'
import { parseArgs, parseEnvString } from '@/lib/utils'
import { FormCard, FormField, FormRow } from '@/components/FormLayout'
import { FolderBrowser } from '@/components/FolderBrowser'
import type { AppSettings } from '@/lib/settings'
import { browseBtnStyle, inputStyle, primaryBtnStyle } from './formStyles'
import type { EnvFileEntry } from '@/types'
import { envFileBg, envFileColor } from '@/lib/envFiles'

interface Props {
  onDone: () => void
  settings: AppSettings
}

export default function StartPage({ onDone, settings }: Props) {
  const [script, setScript] = useState('')
  const [name, setName] = useState('')
  const [cwd, setCwd] = useState('')
  const [namespace, setNamespace] = useState(settings.defaultNamespace || 'default')
  const [args, setArgs] = useState('')
  const [env, setEnv] = useState('')
  const [autorestart, setAutorestart] = useState(true)
  const [watch, setWatch] = useState(false)
  const [cron, setCron] = useState('')
  const [error, setError] = useState('')
  const [loading, setLoading] = useState(false)
  const [envStatus, setEnvStatus] = useState<{ exists: boolean } | null>(null)
  const [envFiles, setEnvFiles] = useState<EnvFileEntry[]>([])
  const [browseOpen, setBrowseOpen] = useState(false)

  // @group BusinessLogic > EnvSidebar : Env file viewer state
  const [activeEnvTab, setActiveEnvTab] = useState<string>('.env')
  const [envContent, setEnvContent] = useState<string>('')
  const [envDirty, setEnvDirty] = useState(false)
  const [envSaved, setEnvSaved] = useState(false)
  const [envSaving, setEnvSaving] = useState(false)
  const [envLoadingFile, setEnvLoadingFile] = useState(false)
  const [envFileError, setEnvFileError] = useState<string | null>(null)

  const envCheckTimer = useRef<ReturnType<typeof setTimeout> | null>(null)
  const envDirectoryRequest = useRef(0)
  const envFileRequest = useRef(0)

  // @group BusinessLogic > EnvCheck : Debounced env file list check when cwd changes
  function handleCwdChange(val: string) {
    if (
      envDirty &&
      val !== cwd &&
      !window.confirm('当前环境文件有未保存的更改。要放弃更改并切换工作目录吗？')
    ) {
      return
    }
    const requestVersion = ++envDirectoryRequest.current
    envFileRequest.current += 1
    setCwd(val)
    setEnvStatus(null)
    setEnvFiles([])
    setEnvContent('')
    setEnvDirty(false)
    setEnvFileError(null)
    if (envCheckTimer.current) clearTimeout(envCheckTimer.current)
    const trimmed = val.trim()
    if (!trimmed) return
    envCheckTimer.current = setTimeout(() => {
      api
        .listEnvPath(trimmed)
        .then(r => {
          if (requestVersion !== envDirectoryRequest.current) return
          setEnvFileError(null)
          setEnvFiles(r.files)
          setEnvStatus({ exists: r.files.some(f => f.name === '.env') })
          if (r.files.length > 0) {
            const first = r.files[0].name
            setActiveEnvTab(first)
            loadEnvFile(first, r.files, requestVersion)
          }
        })
        .catch(listError => {
          const listErrorMessage =
            listError instanceof Error ? listError.message : '无法列出环境文件'
          api
            .checkEnvPath(trimmed)
            .then(r => {
              if (requestVersion !== envDirectoryRequest.current) return
              setEnvStatus({ exists: r.exists })
              setEnvFileError(`环境文件列表读取失败：${listErrorMessage}`)
            })
            .catch(error => {
              if (requestVersion !== envDirectoryRequest.current) return
              setEnvFileError(error instanceof Error ? error.message : '无法检查环境文件')
            })
        })
    }, 500)
  }

  function loadEnvFile(
    filename: string,
    fileList?: EnvFileEntry[],
    directoryVersion = envDirectoryRequest.current
  ) {
    const files = fileList ?? envFiles
    const entry = files.find(f => f.name === filename)
    if (!entry?.path) return
    const fileVersion = ++envFileRequest.current
    setEnvLoadingFile(true)
    setEnvContent('')
    setEnvDirty(false)
    setEnvFileError(null)
    api
      .readEnvFile(entry.path)
      .then(r => {
        if (
          directoryVersion !== envDirectoryRequest.current ||
          fileVersion !== envFileRequest.current
        )
          return
        setEnvContent(r.content)
        setEnvLoadingFile(false)
        setEnvFileError(null)
      })
      .catch(error => {
        if (
          directoryVersion !== envDirectoryRequest.current ||
          fileVersion !== envFileRequest.current
        )
          return
        setEnvLoadingFile(false)
        setEnvFileError(error instanceof Error ? error.message : '读取环境文件失败')
      })
  }

  function switchEnvTab(name: string) {
    if (envDirty && !window.confirm('有未保存的更改，是否放弃并切换？')) return
    setActiveEnvTab(name)
    loadEnvFile(name)
  }

  async function saveEnvFile() {
    const entry = envFiles.find(f => f.name === activeEnvTab)
    if (!entry?.path) return
    setEnvSaving(true)
    setEnvFileError(null)
    try {
      await api.writeEnvFile(entry.path, envContent)
      setEnvDirty(false)
      setEnvSaved(true)
      setTimeout(() => setEnvSaved(false), 2500)
    } catch (error) {
      setEnvFileError(error instanceof Error ? error.message : '保存环境文件失败')
    } finally {
      setEnvSaving(false)
    }
  }

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault()
    setError('')
    setLoading(true)
    try {
      const cronVal = cron.trim() || undefined
      await api.startProcess({
        script: script.trim(),
        ...(name.trim() && { name: name.trim() }),
        ...(cwd.trim() && { cwd: cwd.trim() }),
        ...(namespace.trim() && { namespace: namespace.trim() }),
        ...(args.trim() && { args: parseArgs(args.trim()) }),
        ...(env.trim() && { env: parseEnvString(env.trim()) }),
        autorestart,
        watch,
        ...(cronVal && { cron: cronVal }),
      })
      onDone()
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : '启动进程失败')
    } finally {
      setLoading(false)
    }
  }

  const activeColor = envFileColor(activeEnvTab)
  const showEnvSidebar = envFiles.length > 0

  return (
    <div style={{ display: 'flex', height: '100%', overflow: 'hidden' }}>
      {/* Main form area */}
      <div style={{ flex: 1, overflowY: 'auto', padding: '20px 24px' }}>
        {browseOpen && (
          <FolderBrowser
            initialPath={cwd.trim()}
            onSelect={path => handleCwdChange(path)}
            onClose={() => setBrowseOpen(false)}
          />
        )}
        <div style={{ marginBottom: 20 }}>
          <h2 style={{ fontSize: 16, fontWeight: 600 }}>启动新进程</h2>
        </div>

        <FormCard onSubmit={handleSubmit}>
          <FormRow>
            <FormField label="命令 *">
              <input
                style={inputStyle}
                value={script}
                onChange={e => setScript(e.target.value)}
                placeholder="node app.js"
                required
              />
            </FormField>
            <FormField label="名称">
              <input
                style={inputStyle}
                value={name}
                onChange={e => setName(e.target.value)}
                placeholder="my-app"
              />
            </FormField>
          </FormRow>
          <FormRow>
            <FormField
              associate={false}
              label={
                <span style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                  工作目录
                  {envStatus !== null && (
                    <span
                      style={{
                        fontSize: 10,
                        padding: '1px 6px',
                        borderRadius: 4,
                        fontWeight: 500,
                        background: envStatus.exists
                          ? 'rgba(100,200,100,0.15)'
                          : 'rgba(128,128,128,0.1)',
                        color: envStatus.exists
                          ? 'var(--color-status-running, #4ade80)'
                          : 'var(--color-muted-foreground)',
                      }}
                    >
                      {envStatus.exists ? `● 已找到 .env` : '○ 没有 .env'}
                    </span>
                  )}
                  {envFiles.length > 1 && (
                    <span
                      style={{
                        fontSize: 10,
                        padding: '1px 6px',
                        borderRadius: 4,
                        background: 'rgba(96,165,250,0.13)',
                        color: '#60a5fa',
                        fontWeight: 500,
                      }}
                    >
                      {envFiles.length} 个环境文件
                    </span>
                  )}
                </span>
              }
            >
              <div style={{ display: 'flex', gap: 6 }}>
                <input
                  aria-label="工作目录"
                  style={{ ...inputStyle, flex: 1 }}
                  value={cwd}
                  onChange={e => handleCwdChange(e.target.value)}
                  placeholder="C:\Users\me\app"
                />
                <button
                  type="button"
                  onClick={() => setBrowseOpen(true)}
                  title="浏览文件夹"
                  style={browseBtnStyle}
                >
                  <FolderOpen size={14} strokeWidth={1.75} />
                </button>
              </div>
            </FormField>
            <FormField label="命名空间">
              <NamespaceInput
                style={inputStyle}
                value={namespace}
                onChange={setNamespace}
                placeholder="default"
              />
            </FormField>
          </FormRow>
          <FormRow>
            <FormField label="参数（用空格分隔）">
              <input
                style={inputStyle}
                value={args}
                onChange={e => setArgs(e.target.value)}
                placeholder="--port 3000 --env prod"
              />
            </FormField>
            <FormField label="环境变量（KEY=VAL，用逗号分隔）">
              <input
                style={inputStyle}
                value={env}
                onChange={e => setEnv(e.target.value)}
                placeholder="NODE_ENV=production,PORT=3000"
              />
            </FormField>
          </FormRow>
          <FormRow>
            <FormField label="" associate={false}>
              <div style={{ display: 'flex', gap: 20, marginTop: 4 }}>
                <CheckboxField
                  label="崩溃后自动重启"
                  checked={autorestart}
                  onChange={setAutorestart}
                />
                <CheckboxField label="监视模式" checked={watch} onChange={setWatch} />
              </div>
            </FormField>
            <FormField
              label={
                <>
                  定时任务计划{' '}
                  <span style={{ color: 'var(--color-muted-foreground)', fontSize: 11 }}>
                    （例如“0 * * * *” — 留空则按普通进程运行）
                  </span>
                </>
              }
            >
              <input
                style={inputStyle}
                value={cron}
                onChange={e => setCron(e.target.value)}
                placeholder="0 * * * *"
              />
            </FormField>
          </FormRow>
          <div style={{ display: 'flex', alignItems: 'center', gap: 12, marginTop: 8 }}>
            <button type="submit" disabled={loading} style={primaryBtnStyle}>
              {loading ? '启动中…' : '▶ 启动'}
            </button>
            {error && (
              <span style={{ fontSize: 12, color: 'var(--color-destructive)' }}>{error}</span>
            )}
          </div>
        </FormCard>
      </div>

      {/* @group BusinessLogic > EnvSidebar : Right sidebar showing env files when cwd has them */}
      {showEnvSidebar && (
        <div
          style={{
            width: 360,
            flexShrink: 0,
            borderLeft: '1px solid var(--color-border)',
            display: 'flex',
            flexDirection: 'column',
            background: 'var(--color-card)',
          }}
        >
          {/* Sidebar header */}
          <div
            style={{
              padding: '10px 14px 0',
              borderBottom: '1px solid var(--color-border)',
              flexShrink: 0,
            }}
          >
            <div style={{ display: 'flex', alignItems: 'center', gap: 6, paddingBottom: 8 }}>
              <span style={{ fontSize: 13 }}>🔑</span>
              <span style={{ fontWeight: 600, fontSize: 12, flex: 1 }}>环境文件</span>
              <span style={{ fontSize: 11, color: 'var(--color-muted-foreground)' }}>仅预览</span>
            </div>

            {/* Tabs */}
            <div style={{ display: 'flex', gap: 2, overflowX: 'auto' }}>
              {envFiles.map(f => {
                const isActive = f.name === activeEnvTab
                const color = envFileColor(f.name)
                const bg = envFileBg(f.name)
                return (
                  <button
                    key={f.name}
                    onClick={() => switchEnvTab(f.name)}
                    style={{
                      padding: '4px 10px',
                      fontSize: 11,
                      fontWeight: isActive ? 700 : 500,
                      background: isActive ? bg : 'transparent',
                      border: 'none',
                      borderBottom: isActive ? `2px solid ${color}` : '2px solid transparent',
                      borderRadius: '3px 3px 0 0',
                      cursor: 'pointer',
                      color: isActive ? color : 'var(--color-muted-foreground)',
                      whiteSpace: 'nowrap',
                      flexShrink: 0,
                    }}
                  >
                    {f.name}
                  </button>
                )
              })}
            </div>
          </div>

          {/* Editor area */}
          <div
            style={{
              flex: 1,
              overflow: 'hidden',
              display: 'flex',
              flexDirection: 'column',
              padding: '8px 12px',
              gap: 6,
            }}
          >
            {envLoadingFile ? (
              <div
                style={{
                  color: 'var(--color-muted-foreground)',
                  fontSize: 13,
                  padding: 20,
                  textAlign: 'center',
                }}
              >
                加载中…
              </div>
            ) : (
              <>
                <div
                  style={{
                    flex: 1,
                    display: 'flex',
                    gap: 0,
                    overflow: 'hidden',
                    border: `1px solid ${envDirty ? activeColor : 'var(--color-border)'}`,
                    borderRadius: 4,
                    background: 'var(--color-background)',
                  }}
                >
                  {/* Line numbers */}
                  <div
                    style={{
                      padding: '8px 6px',
                      textAlign: 'right',
                      userSelect: 'none',
                      fontFamily: 'monospace',
                      fontSize: 11,
                      lineHeight: '1.6',
                      color: 'var(--color-muted-foreground)',
                      background: 'var(--color-muted)',
                      borderRight: '1px solid var(--color-border)',
                      minWidth: 28,
                      overflowY: 'hidden',
                    }}
                  >
                    {envContent.split('\n').map((_, i) => (
                      <div key={i}>{i + 1}</div>
                    ))}
                  </div>
                  <textarea
                    value={envContent}
                    onChange={e => {
                      setEnvContent(e.target.value)
                      setEnvDirty(true)
                      setEnvSaved(false)
                    }}
                    spellCheck={false}
                    placeholder="KEY=value"
                    style={{
                      flex: 1,
                      padding: '8px 10px',
                      fontFamily: 'monospace',
                      fontSize: 11,
                      lineHeight: '1.6',
                      background: 'transparent',
                      color: 'var(--color-foreground)',
                      border: 'none',
                      outline: 'none',
                      resize: 'none',
                      minHeight: 0,
                    }}
                  />
                </div>
              </>
            )}
          </div>

          {/* Footer */}
          <div
            style={{
              padding: '8px 12px',
              borderTop: '1px solid var(--color-border)',
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'space-between',
              flexShrink: 0,
            }}
          >
            <span style={{ fontSize: 11, color: 'var(--color-muted-foreground)' }}>
              {envFileError ? (
                <span style={{ color: 'var(--color-destructive)' }}>{envFileError}</span>
              ) : envDirty ? (
                <span style={{ color: activeColor }}>● 未保存</span>
              ) : envSaved ? (
                '✓ 已保存'
              ) : (
                `${activeEnvTab}`
              )}
            </span>
            <button
              disabled={envSaving || envLoadingFile || !envDirty}
              onClick={saveEnvFile}
              style={{
                padding: '4px 12px',
                fontSize: 11,
                fontWeight: 600,
                cursor: 'pointer',
                background: activeColor,
                border: 'none',
                borderRadius: 5,
                color: '#000',
                opacity: envSaving || envLoadingFile || !envDirty ? 0.5 : 1,
              }}
            >
              {envSaving ? '保存中…' : '保存'}
            </button>
          </div>
        </div>
      )}
    </div>
  )
}

function CheckboxField({
  label,
  checked,
  onChange,
}: {
  label: string
  checked: boolean
  onChange: (v: boolean) => void
}) {
  return (
    <label
      style={{ display: 'flex', alignItems: 'center', gap: 6, cursor: 'pointer', fontSize: 13 }}
    >
      <input
        type="checkbox"
        checked={checked}
        onChange={e => onChange(e.target.checked)}
        style={{ accentColor: 'var(--color-primary)', width: 14, height: 14 }}
      />
      {label}
    </label>
  )
}
