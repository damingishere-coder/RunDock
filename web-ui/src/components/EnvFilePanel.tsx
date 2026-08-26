// @group BusinessLogic : .env file viewer and editor — inline side-panel variant
// Renders in a flex column; no overlay. Used by ProcessesPage split view.
// Supports multiple env file tabs with color coding and key-sync across versions.

import { useCallback, useEffect, useRef, useState } from 'react'
import { RefreshCw } from 'lucide-react'
import { api } from '@/lib/api'
import type { EnvFileEntry } from '@/types'
import { EnvEditor } from '@/components/EnvEditor'
import { envFileBg, envFileColor } from '@/lib/envFiles'

interface Props {
  processId: string
  processName: string
  onClose: () => void
  onRestart: () => void
}

export function EnvFilePanel({ processId, processName, onClose, onRestart }: Props) {
  // @group BusinessLogic > State : Tab and file management
  const [files, setFiles] = useState<EnvFileEntry[]>([])
  const [activeTab, setActiveTab] = useState<string>('.env')
  const [content, setContent] = useState('')
  const [exists, setExists] = useState(false)
  const [loadingList, setLoadingList] = useState(true)
  const [listFailed, setListFailed] = useState(false)
  const [listRetry, setListRetry] = useState(0)
  const [loadingFile, setLoadingFile] = useState(false)
  const [saving, setSaving] = useState(false)
  const [syncing, setSyncing] = useState(false)
  const [saved, setSaved] = useState(false)
  const [dirty, setDirty] = useState(false)
  const [error, setError] = useState('')
  const [syncMsg, setSyncMsg] = useState('')
  const textareaRef = useRef<HTMLTextAreaElement>(null)
  const fileRequestRef = useRef(0)
  const fileAbortRef = useRef<AbortController | null>(null)
  const mutationRef = useRef(false)

  const loadFile = useCallback(
    (filename: string) => {
      const requestId = ++fileRequestRef.current
      fileAbortRef.current?.abort()
      const controller = new AbortController()
      fileAbortRef.current = controller
      setLoadingFile(true)
      setContent('')
      setDirty(false)
      setSaved(false)
      setError('')
      api
        .getEnvFile(processId, filename, { signal: controller.signal })
        .then(data => {
          if (requestId !== fileRequestRef.current || controller.signal.aborted) return
          setContent(data.content)
          setExists(data.exists)
          setLoadingFile(false)
          setTimeout(() => textareaRef.current?.focus(), 50)
        })
        .catch((error: unknown) => {
          if (requestId !== fileRequestRef.current || controller.signal.aborted) return
          setError(error instanceof Error ? error.message : String(error))
          setLoadingFile(false)
        })
    },
    [processId]
  )

  // @group BusinessLogic : Load file list when process changes
  useEffect(() => {
    const controller = new AbortController()
    setLoadingList(true)
    setListFailed(false)
    setFiles([])
    setContent('')
    setDirty(false)
    setSaved(false)
    setError('')
    setSyncMsg('')
    api
      .listEnvFiles(processId, { signal: controller.signal })
      .then(data => {
        if (controller.signal.aborted) return
        setFiles(data.files)
        setListFailed(false)
        const first = data.files[0]?.name ?? '.env'
        setActiveTab(first)
        setLoadingList(false)
        loadFile(first)
      })
      .catch((error: unknown) => {
        if (controller.signal.aborted) return
        setFiles([])
        setLoadingList(false)
        setListFailed(true)
        setError(error instanceof Error ? error.message : '无法读取环境文件列表')
      })
    return () => {
      controller.abort()
      fileRequestRef.current += 1
      fileAbortRef.current?.abort()
      fileAbortRef.current = null
    }
  }, [processId, loadFile, listRetry])

  function switchTab(name: string) {
    if (mutationRef.current) return
    if (dirty) {
      if (!window.confirm('有未保存的更改。要放弃更改并切换文件吗？')) return
    }
    setActiveTab(name)
    loadFile(name)
  }

  const requestClose = useCallback(() => {
    if (mutationRef.current) return
    if (dirty && !window.confirm('有未保存的更改。要放弃更改并关闭编辑器吗？')) return
    onClose()
  }, [dirty, onClose])

  async function handleSave(andRestart: boolean) {
    if (mutationRef.current) return
    mutationRef.current = true
    setSaving(true)
    setError('')
    let fileSaved = false
    try {
      await api.saveEnvFile(processId, content, activeTab)
      fileSaved = true
      setExists(true)
      setDirty(false)
      setSaved(true)
      if (andRestart) {
        await api.restartProcess(processId)
        onRestart()
        onClose()
      } else {
        setTimeout(() => setSaved(false), 2500)
      }
    } catch (e: unknown) {
      const detail = e instanceof Error ? e.message : String(e)
      setError(fileSaved && andRestart ? `.env 已保存，但进程重启失败：${detail}` : detail)
    } finally {
      mutationRef.current = false
      setSaving(false)
    }
  }

  // @group BusinessLogic > Sync : Propagate keys from active file to all other env files
  async function handleSync() {
    if (mutationRef.current) return
    mutationRef.current = true
    setSyncing(true)
    setSyncMsg('')
    setError('')
    try {
      // First save the current file so sync reads fresh content.
      await api.saveEnvFile(processId, content, activeTab)
      setExists(true)
      setDirty(false)
      const activeFile = files.find(f => f.name === activeTab)
      if (!activeFile?.path) throw new Error('无法同步：文件路径未知')
      const result = await api.syncEnvFiles(activeFile.path)
      if (result.success) {
        setSyncMsg(`✓ 已将键同步到 ${result.synced_files} 个文件`)
      } else {
        setSyncMsg(
          `已同步 ${result.synced_files} 个文件${result.errors?.length ? `（${result.errors.length} 个错误）` : ''}`
        )
      }
      setTimeout(() => setSyncMsg(''), 4000)
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      mutationRef.current = false
      setSyncing(false)
    }
  }

  const lineCount = content.split('\n').length
  const activeColor = envFileColor(activeTab)
  const hasMultipleFiles = files.length > 1
  const mutating = saving || syncing

  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        height: '100%',
        background: 'var(--color-card)',
      }}
    >
      {/* Header */}
      <div
        style={{
          padding: '10px 14px 0',
          borderBottom: '1px solid var(--color-border)',
          flexShrink: 0,
        }}
      >
        <div style={{ display: 'flex', alignItems: 'center', gap: 8, paddingBottom: 8 }}>
          <span style={{ fontSize: 14 }}>🔑</span>
          <span
            style={{
              fontWeight: 600,
              fontSize: 13,
              flex: 1,
              minWidth: 0,
              overflow: 'hidden',
              textOverflow: 'ellipsis',
              whiteSpace: 'nowrap',
            }}
          >
            {processName}
          </span>
          <button
            onClick={requestClose}
            aria-label="关闭环境变量编辑器"
            style={{
              background: 'none',
              border: 'none',
              cursor: 'pointer',
              fontSize: 16,
              lineHeight: 1,
              color: 'var(--color-muted-foreground)',
              padding: '0 2px',
              flexShrink: 0,
            }}
          >
            ×
          </button>
        </div>

        {/* Tabs */}
        {!loadingList && !listFailed && (
          <div style={{ display: 'flex', gap: 2, overflowX: 'auto', paddingBottom: 0 }}>
            {(files.length > 0 ? files : [{ name: '.env', path: '' }]).map(f => {
              const isActive = f.name === activeTab
              const color = envFileColor(f.name)
              const bg = envFileBg(f.name)
              return (
                <button
                  key={f.name}
                  onClick={() => switchTab(f.name)}
                  disabled={mutating}
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
                    transition: 'color 0.1s',
                  }}
                >
                  {f.name}
                </button>
              )
            })}
          </div>
        )}
      </div>

      {/* Body */}
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
        {loadingList || loadingFile ? (
          <div
            style={{
              color: 'var(--color-muted-foreground)',
              padding: 24,
              textAlign: 'center',
              fontSize: 13,
            }}
          >
            加载中…
          </div>
        ) : listFailed ? (
          <div role="alert" style={{ padding: 16, color: 'var(--color-destructive)' }}>
            <div>{error || '无法读取环境文件列表'}</div>
            <button type="button" onClick={() => setListRetry(value => value + 1)}>
              重试
            </button>
          </div>
        ) : (
          <>
            {!exists && (
              <div
                style={{
                  fontSize: 12,
                  padding: '5px 8px',
                  borderRadius: 4,
                  background: 'var(--color-muted)',
                  color: 'var(--color-muted-foreground)',
                  borderLeft: `3px solid ${activeColor}`,
                }}
              >
                未找到 <code>{activeTab}</code>。保存时将创建该文件。
              </div>
            )}

            {/* Editor */}
            <EnvEditor
              value={content}
              onChange={v => {
                setContent(v)
                setDirty(true)
                setSaved(false)
              }}
              borderColor={dirty ? activeColor : 'var(--color-border)'}
              placeholder={'KEY=value\nDATABASE_URL=postgres://...\nSECRET_KEY=...'}
              textareaRef={textareaRef}
              disabled={mutating}
            />

            {syncMsg && (
              <div style={{ fontSize: 11, color: activeColor, padding: '3px 0' }}>{syncMsg}</div>
            )}
            {error && (
              <div
                style={{
                  fontSize: 12,
                  color: 'var(--color-destructive)',
                  padding: '5px 8px',
                  borderRadius: 4,
                  background: 'rgba(255,100,100,0.1)',
                }}
              >
                {error}
              </div>
            )}
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
          gap: 6,
          flexShrink: 0,
          flexWrap: 'wrap',
        }}
      >
        <span style={{ fontSize: 11, color: 'var(--color-muted-foreground)', flexShrink: 0 }}>
          {dirty ? (
            <span style={{ color: activeColor }}>● 未保存</span>
          ) : saved ? (
            '✓ 已保存'
          ) : (
            `${lineCount} 行`
          )}
        </span>
        <div style={{ display: 'flex', gap: 5 }}>
          {hasMultipleFiles && (
            <button
              disabled={mutating || loadingFile}
              onClick={handleSync}
              title="将此文件中的键同步到其他环境文件"
              style={{
                ...cancelBtnStyle,
                display: 'flex',
                alignItems: 'center',
                gap: 4,
                opacity: mutating || loadingFile ? 0.6 : 1,
              }}
            >
              <RefreshCw size={11} strokeWidth={2} />
              {syncing ? '同步中…' : '同步键'}
            </button>
          )}
          <button
            disabled={mutating || loadingFile}
            onClick={() => handleSave(false)}
            style={{ ...cancelBtnStyle, opacity: mutating || loadingFile ? 0.6 : 1 }}
          >
            {saving ? '保存中…' : '保存'}
          </button>
          <button
            disabled={mutating || loadingFile}
            onClick={() => handleSave(true)}
            style={{ ...primaryBtnStyle(activeColor), opacity: mutating || loadingFile ? 0.6 : 1 }}
          >
            {saving ? '保存中…' : '保存并重启 ↺'}
          </button>
        </div>
      </div>
    </div>
  )
}

// @group Utilities > Styles
const cancelBtnStyle: React.CSSProperties = {
  padding: '4px 10px',
  fontSize: 11,
  cursor: 'pointer',
  background: 'var(--color-secondary)',
  border: '1px solid var(--color-border)',
  borderRadius: 5,
  color: 'var(--color-foreground)',
}

function primaryBtnStyle(color: string): React.CSSProperties {
  return {
    padding: '4px 10px',
    fontSize: 11,
    fontWeight: 600,
    cursor: 'pointer',
    background: color,
    border: `1px solid ${color}`,
    borderRadius: 5,
    color: '#000',
  }
}
