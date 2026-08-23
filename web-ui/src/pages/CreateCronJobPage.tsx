// @group BusinessLogic : Create cron job — split panel with inline code editor + schedule settings

import { useState, useRef, useCallback } from 'react'
import { api } from '@/lib/api'
import { parseEnvString } from '@/lib/utils'
import { NamespaceInput } from '@/components/NamespaceInput'
import { CronExpressionInput } from '@/components/CronExpressionInput'
import { CodeEditor } from '@/components/CodeEditor'
import { RunOutput } from '@/components/RunOutput'
import type { OutputLine } from '@/components/RunOutput'
import { inputStyle, primaryBtnStyle } from './StartPage'
import type { AppSettings } from '@/lib/settings'

interface Props {
  onDone: () => void
  settings: AppSettings
}

// @group Configuration > Interpreters : Supported interpreter presets
const INTERPRETERS = [
  // @group Configuration > Interpreters > Python
  { label: 'Python',                      value: 'python' },
  { label: 'Python 3',                    value: 'python3' },
  // @group Configuration > Interpreters > JavaScript / TypeScript
  { label: 'Node.js',                     value: 'node' },
  { label: 'Bun',                         value: 'bun' },
  { label: 'Deno',                        value: 'deno' },
  { label: 'ts-node',                     value: 'ts-node' },
  // @group Configuration > Interpreters > Shell
  { label: 'Bash',                        value: 'bash' },
  { label: 'Sh',                          value: 'sh' },
  { label: 'Zsh',                         value: 'zsh' },
  { label: 'Fish',                        value: 'fish' },
  { label: 'PowerShell',                  value: 'powershell' },
  { label: 'PowerShell Core',             value: 'pwsh' },
  { label: 'Cmd (Windows)',               value: 'cmd' },
  // @group Configuration > Interpreters > Ruby
  { label: 'Ruby',                        value: 'ruby' },
  // @group Configuration > Interpreters > PHP
  { label: 'PHP',                         value: 'php' },
  // @group Configuration > Interpreters > Perl
  { label: 'Perl',                        value: 'perl' },
  // @group Configuration > Interpreters > Lua
  { label: 'Lua',                         value: 'lua' },
  // @group Configuration > Interpreters > Java / JVM
  { label: 'Java',                        value: 'java' },
  { label: 'Groovy',                      value: 'groovy' },
  { label: 'Kotlin',                      value: 'kotlin' },
  { label: 'Scala',                       value: 'scala' },
  { label: 'Clojure (clj)',               value: 'clj' },
  // @group Configuration > Interpreters > .NET
  { label: 'C# Script (dotnet-script)',   value: 'dotnet-script' },
  { label: 'dotnet  (fsi / run / …)',     value: 'dotnet' },
  // @group Configuration > Interpreters > Go
  { label: 'Go run',                      value: 'go' },
  // @group Configuration > Interpreters > Rust
  { label: 'Rust (cargo-script)',         value: 'cargo-script' },
  // @group Configuration > Interpreters > R
  { label: 'Rscript',                     value: 'Rscript' },
  // @group Configuration > Interpreters > Julia
  { label: 'Julia',                       value: 'julia' },
  // @group Configuration > Interpreters > Swift
  { label: 'Swift',                       value: 'swift' },
  // @group Configuration > Interpreters > Elixir
  { label: 'Elixir',                      value: 'elixir' },
  // @group Configuration > Interpreters > Erlang
  { label: 'Escript (Erlang)',            value: 'escript' },
  // @group Configuration > Interpreters > Haskell
  { label: 'Haskell (runghc)',            value: 'runghc' },
  // @group Configuration > Interpreters > OCaml
  { label: 'OCaml',                       value: 'ocaml' },
  // @group Configuration > Interpreters > Tcl
  { label: 'Tcl',                         value: 'tclsh' },
  // @group Configuration > Interpreters > AWK
  { label: 'AWK',                         value: 'awk' },
  // @group Configuration > Interpreters > Custom
  { label: 'Custom…',                     value: '__custom__' },
]

// @group Utilities > LangLabel : Human-readable language label from interpreter value
function langLabel(value: string): string {
  return INTERPRETERS.find(i => i.value === value)?.label ?? value
}

export default function CreateCronJobPage({ onDone, settings }: Props) {
  // @group BusinessLogic > State : Left panel — editor state
  const [interpreter, setInterpreter]       = useState('python')
  const [customInterpreter, setCustomInterp] = useState('')
  const [scriptName, setScriptName]         = useState('')
  const [code, setCode]                     = useState('')
  const [savedName, setSavedName]           = useState<string | null>(null)
  const [isSaving, setIsSaving]             = useState(false)
  const [isRunning, setIsRunning]           = useState(false)
  const [runLines, setRunLines]             = useState<OutputLine[]>([])
  const [runExitCode, setRunExitCode]       = useState<number | null | undefined>(undefined)
  const [saveError, setSaveError]           = useState('')

  // @group BusinessLogic > State : Right panel — schedule + settings
  const [cron, setCron]           = useState('')
  const [cwd, setCwd]             = useState('')
  const [envStr, setEnvStr]       = useState('')
  const [namespace, setNamespace] = useState(settings.defaultNamespace || 'default')
  const [argsStr, setArgsStr]     = useState('')
  const [jobName, setJobName]     = useState('')
  const [submitError, setSubmitError] = useState('')
  const [loading, setLoading]     = useState(false)

  const esRef = useRef<EventSource | null>(null)

  const effectiveInterpreter = interpreter === '__custom__' ? customInterpreter.trim() : interpreter

  // @group BusinessLogic > Save : POST /api/v1/scripts to save code to daemon disk
  const handleSave = useCallback(async () => {
    if (!code.trim()) { setSaveError('请先编写代码。'); return }
    const name = scriptName.trim() || 'script'
    setSaveError('')
    setIsSaving(true)
    try {
      const res = await api.saveScript({ name, language: effectiveInterpreter, content: code })
      setSavedName(res.name)
    } catch (e: unknown) {
      setSaveError(e instanceof Error ? e.message : '保存脚本失败')
    } finally {
      setIsSaving(false)
    }
  }, [code, scriptName, effectiveInterpreter])

  // @group BusinessLogic > Run : Stream script output via SSE
  const handleRun = useCallback(async () => {
    // Auto-save first if needed
    let name = savedName
    if (!name) {
      if (!code.trim()) { setSaveError('请先编写代码。'); return }
      setSaveError('')
      setIsSaving(true)
      try {
        const res = await api.saveScript({
          name: scriptName.trim() || 'script',
          language: effectiveInterpreter,
          content: code,
        })
        name = res.name
        setSavedName(res.name)
      } catch (e: unknown) {
        setSaveError(e instanceof Error ? e.message : '保存脚本失败')
        setIsSaving(false)
        return
      } finally {
        setIsSaving(false)
      }
    }

    // Close previous EventSource if any
    esRef.current?.close()
    setRunLines([])
    setRunExitCode(undefined)
    setIsRunning(true)

    const es = api.runScript(name!)
    esRef.current = es

    es.onmessage = (evt) => {
      try {
        const data = JSON.parse(evt.data)
        if (data.done) {
          setRunExitCode(data.exit_code ?? null)
          setIsRunning(false)
          es.close()
        } else {
          setRunLines(prev => [...prev, {
            stream: data.stream as 'stdout' | 'stderr',
            content: data.content as string,
          }])
        }
      } catch { /* ignore parse errors */ }
    }

    es.onerror = () => {
      setIsRunning(false)
      es.close()
    }
  }, [savedName, code, scriptName, effectiveInterpreter])

  // @group BusinessLogic > Submit : Create the cron job process using saved script path
  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault()
    if (!effectiveInterpreter) { setSubmitError('请选择或输入解释器。'); return }
    if (!savedName) { setSubmitError('请先保存脚本，再创建定时任务。'); return }
    if (!cron.trim()) { setSubmitError('必须填写定时任务计划。'); return }
    setSubmitError('')
    setLoading(true)
    try {
      // Get the saved script's full path from daemon
      const scriptInfo = await api.getScript(savedName)
      const extraArgs = argsStr.trim() ? argsStr.trim().split(/\s+/) : []
      const args = [scriptInfo.path, ...extraArgs]
      await api.startProcess({
        script: effectiveInterpreter,
        args,
        name: jobName.trim() || savedName,
        ...(cwd.trim()    && { cwd: cwd.trim() }),
        namespace: namespace.trim() || 'default',
        ...(envStr.trim() && { env: parseEnvString(envStr.trim()) }),
        cron: cron.trim(),
        autorestart: false,
      })
      onDone()
    } catch (err: unknown) {
      setSubmitError(err instanceof Error ? err.message : '创建定时任务失败')
    } finally {
      setLoading(false)
    }
  }

  const cancelBtn: React.CSSProperties = {
    padding: '7px 16px', fontSize: 13,
    background: 'transparent', border: '1px solid var(--color-border)',
    borderRadius: 5, cursor: 'pointer', color: 'var(--color-muted-foreground)',
  }

  const fieldLabel: React.CSSProperties = {
    display: 'block', fontSize: 11, fontWeight: 600,
    color: 'var(--color-muted-foreground)', marginBottom: 5, letterSpacing: '0.04em',
    textTransform: 'uppercase',
  }

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%', overflow: 'hidden' }}>

      {/* Page header */}
      <div style={{
        padding: '14px 20px 10px',
        borderBottom: '1px solid var(--color-border)',
        display: 'flex', alignItems: 'center', justifyContent: 'space-between',
        flexShrink: 0,
      }}>
        <div>
          <h2 style={{ fontSize: 15, fontWeight: 600 }}>新建定时任务</h2>
          <p style={{ fontSize: 12, color: 'var(--color-muted-foreground)', marginTop: 2 }}>
            编写并测试脚本，然后设置运行计划。
          </p>
        </div>
        <button type="button" onClick={onDone} style={cancelBtn}>✕ 取消</button>
      </div>

      {/* Split panel */}
      <div style={{ display: 'flex', flex: 1, overflow: 'hidden' }}>

        {/* ── LEFT PANEL (60%) : Editor + Run output ── */}
        <div style={{
          flex: '0 0 60%', display: 'flex', flexDirection: 'column',
          borderRight: '1px solid var(--color-border)',
          overflow: 'hidden',
        }}>
          {/* Editor toolbar */}
          <div style={{
            padding: '10px 16px 8px',
            borderBottom: '1px solid var(--color-border)',
            display: 'flex', gap: 8, alignItems: 'flex-end', flexShrink: 0,
            flexWrap: 'wrap',
          }}>
            {/* Interpreter */}
            <div style={{ flex: '0 0 auto' }}>
              <label style={fieldLabel}>解释器</label>
              <select
                value={interpreter}
                onChange={e => { setInterpreter(e.target.value); setSavedName(null) }}
                style={{ ...inputStyle, width: 'auto', minWidth: 140, cursor: 'pointer', fontSize: 12 }}
              >
                {INTERPRETERS.map(i => (
                  <option key={i.value + i.label} value={i.value}>{i.label}</option>
                ))}
              </select>
            </div>

            {/* Custom interpreter input */}
            {interpreter === '__custom__' && (
              <div style={{ flex: '1 1 140px' }}>
                <label style={fieldLabel}>解释器路径</label>
                <input
                  style={{ ...inputStyle, fontSize: 12 }}
                  value={customInterpreter}
                  onChange={e => { setCustomInterp(e.target.value); setSavedName(null) }}
                  placeholder="C:\bin\myinterp.exe"
                />
              </div>
            )}

            {/* Script name */}
            <div style={{ flex: '1 1 120px' }}>
              <label style={fieldLabel}>脚本名称</label>
              <input
                style={{ ...inputStyle, fontSize: 12 }}
                value={scriptName}
                onChange={e => { setScriptName(e.target.value); setSavedName(null) }}
                placeholder="my-script"
              />
            </div>

            {/* Save button */}
            <div style={{ alignSelf: 'flex-end' }}>
              <button
                type="button"
                onClick={handleSave}
                disabled={isSaving || !code.trim()}
                style={{
                  ...primaryBtnStyle,
                  background: savedName ? 'var(--color-status-running)' : 'var(--color-primary)',
                  fontSize: 12, padding: '6px 14px',
                  opacity: !code.trim() ? 0.5 : 1,
                }}
              >
                {isSaving ? '保存中…' : savedName ? '✓ 已保存' : '💾 保存'}
              </button>
            </div>

            {/* Run button */}
            <div style={{ alignSelf: 'flex-end' }}>
              <button
                type="button"
                onClick={handleRun}
                disabled={isRunning || !code.trim()}
                style={{
                  ...primaryBtnStyle,
                  background: '#6366f1',
                  fontSize: 12, padding: '6px 14px',
                  opacity: !code.trim() ? 0.5 : 1,
                }}
              >
                {isRunning ? '⏳ 运行中…' : '▶ 运行'}
              </button>
            </div>

            {saveError && (
              <span style={{ fontSize: 11, color: 'var(--color-destructive)', alignSelf: 'flex-end' }}>{saveError}</span>
            )}

            {savedName && (
              <span style={{ fontSize: 11, color: 'var(--color-muted-foreground)', alignSelf: 'flex-end', fontFamily: 'monospace' }}>
                → {savedName}
              </span>
            )}
          </div>

          {/* Code editor + run output */}
          <div style={{ flex: 1, overflow: 'hidden', display: 'flex', flexDirection: 'column', padding: 12, gap: 10 }}>
            <CodeEditor
              value={code}
              onChange={(v) => { setCode(v); setSavedName(null) }}
              language={langLabel(effectiveInterpreter)}
              height="60%"
            />
            <RunOutput
              lines={runLines}
              exitCode={runExitCode}
              isRunning={isRunning}
              onClear={() => { setRunLines([]); setRunExitCode(undefined) }}
              height="40%"
            />
          </div>
        </div>

        {/* ── RIGHT PANEL (40%) : Schedule + settings ── */}
        <div style={{ flex: '0 0 40%', overflowY: 'auto', padding: '16px 20px' }}>
          <form onSubmit={handleSubmit} style={{ display: 'flex', flexDirection: 'column', gap: 16 }}>

            {/* Cron schedule */}
            <div>
              <label style={fieldLabel}>定时任务计划 *</label>
              <CronExpressionInput value={cron} onChange={setCron} />
            </div>

            {/* Job name */}
            <div>
              <label style={fieldLabel}>任务名称</label>
              <input
                style={inputStyle}
                value={jobName}
                onChange={e => setJobName(e.target.value)}
                placeholder={savedName ?? '自动使用脚本名称'}
              />
            </div>

            {/* Working directory */}
            <div>
              <label style={fieldLabel}>工作目录</label>
              <input
                style={inputStyle}
                value={cwd}
                onChange={e => setCwd(e.target.value)}
                placeholder="留空则使用脚本文件夹"
              />
            </div>

            {/* Extra args */}
            <div>
              <label style={fieldLabel}>
                {interpreter === 'dotnet' ? 'dotnet 子命令 + 参数' : '额外参数'}
              </label>
              <input
                style={inputStyle}
                value={argsStr}
                onChange={e => setArgsStr(e.target.value)}
                placeholder={interpreter === 'dotnet' ? 'fsi script.fsx  or  run' : '--verbose --output /tmp'}
              />
            </div>

            {/* Env vars */}
            <div>
              <label style={fieldLabel}>环境变量 <span style={{ fontWeight: 400 }}>（KEY=VAL，用逗号分隔）</span></label>
              <input
                style={inputStyle}
                value={envStr}
                onChange={e => setEnvStr(e.target.value)}
                placeholder="API_KEY=abc,TIMEOUT=30"
              />
            </div>

            {/* Namespace */}
            <div>
              <label style={fieldLabel}>命名空间</label>
              <NamespaceInput style={inputStyle} value={namespace} onChange={setNamespace} placeholder="default" />
            </div>

            {/* Submit */}
            <div style={{ display: 'flex', flexDirection: 'column', gap: 8, paddingTop: 4 }}>
              {!savedName && (
                <div style={{
                  padding: '8px 12px', background: 'rgba(234,179,8,0.1)',
                  border: '1px solid rgba(234,179,8,0.3)', borderRadius: 5,
                  fontSize: 12, color: '#eab308',
                }}>
                  ⚠ 创建定时任务前，请先保存脚本（左侧面板）。
                </div>
              )}
              <button
                type="submit"
                disabled={loading || !savedName}
                style={{
                  ...primaryBtnStyle,
                  opacity: !savedName ? 0.5 : 1,
                  width: '100%', justifyContent: 'center',
                }}
              >
                {loading ? '创建中…' : '⏱ 创建定时任务'}
              </button>
              {submitError && (
                <span style={{ fontSize: 12, color: 'var(--color-destructive)' }}>{submitError}</span>
              )}
            </div>
          </form>
        </div>
      </div>
    </div>
  )
}
