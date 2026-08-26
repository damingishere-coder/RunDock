// @group BusinessLogic : Browser-based terminal panel — multi-tab split-pane PTY over WebSocket

import { forwardRef, useCallback, useEffect, useImperativeHandle, useRef, useState } from 'react'
import { Terminal } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import {
  Maximize2,
  Minimize2,
  X,
  Plus,
  SquareTerminal,
  ChevronDown,
  SquareSplitHorizontal,
  History,
} from 'lucide-react'
import { getActiveServer, serverBaseUrl } from '@/lib/servers'
import { api, type CmdEntry as ApiCmdEntry } from '@/lib/api'
import { waitForAbortableDelay } from '@/lib/async'
import { mergeTerminalHistory, terminalHistoryKey } from '@/lib/terminalHistory'
import '@xterm/xterm/css/xterm.css'

// @group Types : Panel visibility states
export type TerminalPanelState = 'hidden' | 'normal' | 'maximized'

// @group Types : Imperative handle — lets parent components open a tab programmatically
export interface TerminalPanelHandle {
  openTab: (cwd?: string, name?: string) => void
}

// @group Types : Terminal keyboard shortcut config (mirrors AppSettings['terminalShortcuts'])
export interface TerminalShortcuts {
  splitPane: string
  duplicateTab: string
  newTab: string
}

// @group Types : One terminal tab — React state, no xterm objects
interface TerminalTab {
  id: string // primary pane ID + tab unique key
  title: string
  cwd: string
  splitPaneId?: string // if set, a second pane exists side-by-side with the primary
  processKey?: string // stable key for history persistence (e.g. "proc:api-server")
}

// @group Types : Live xterm + WebSocket instance, stored outside React state
interface TerminalInstance {
  term: Terminal
  fitAddon: FitAddon
  ws: WebSocket | null
  connected: boolean
  connectAbort: AbortController
}

// @group Types : Command history entry per pane (matches Rust CmdEntry)
type CmdEntry = ApiCmdEntry

const MAX_TERMINAL_TABS = 8
const MAX_TERMINAL_CONNECT_ATTEMPTS = 8
const MAX_TERMINAL_CONNECT_MS = 2 * 60_000

// @group Utilities > Terminal > History : Load history from daemon data directory (non-blocking)
async function loadPersistedHistory(key: string): Promise<CmdEntry[]> {
  return api.getTerminalHistory(key)
}

// @group Utilities > Terminal > History : Debounced save — coalesces rapid saves to one API call per key
function savePersistedHistory(
  saveTimers: Map<string, ReturnType<typeof setTimeout>>,
  key: string,
  entries: CmdEntry[],
  onError: (message: string) => void
): void {
  const existing = saveTimers.get(key)
  if (existing) clearTimeout(existing)
  const t = setTimeout(() => {
    saveTimers.delete(key)
    api.saveTerminalHistory(key, entries.slice(0, 150)).catch(error => {
      onError(error instanceof Error ? error.message : '保存命令历史失败')
    })
  }, 800)
  saveTimers.set(key, t)
}

interface TerminalPanelProps {
  panelState: TerminalPanelState
  onChangePanelState: (s: TerminalPanelState) => void
  onTabCountChange: (count: number) => void
  shortcuts: TerminalShortcuts
}

// @group Utilities > Terminal : Build WebSocket URL for the active server
async function buildTerminalWsUrl(
  cwd: string,
  cols: number,
  rows: number,
  signal: AbortSignal
): Promise<string> {
  const server = getActiveServer()
  const base = serverBaseUrl(server)
  const query = new URLSearchParams({ cwd, cols: String(cols), rows: String(rows) }).toString()
  const { ticket } = await api.createStreamTicket('/terminals/ws', query, { signal })
  const params = new URLSearchParams(query)
  params.set('ticket', ticket)
  if (base.startsWith('/')) {
    const wsOrigin = window.location.origin.replace(/^http/, 'ws')
    return `${wsOrigin}${base}/terminals/ws?${params}`
  }
  return `${base.replace(/^http/, 'ws')}/terminals/ws?${params}`
}

// @group Utilities > Terminal : Match a KeyboardEvent against a shortcut string like "ctrl+shift+t"
function matchesShortcut(e: KeyboardEvent, shortcut: string): boolean {
  if (!shortcut.trim()) return false
  const parts = shortcut
    .toLowerCase()
    .split('+')
    .map(p => p.trim())
  const key = parts[parts.length - 1]
  return (
    e.key.toLowerCase() === key &&
    e.ctrlKey === parts.includes('ctrl') &&
    e.shiftKey === parts.includes('shift') &&
    e.altKey === parts.includes('alt') &&
    e.metaKey === (parts.includes('meta') || parts.includes('cmd'))
  )
}

// @group BusinessLogic : TerminalPanel — floating multi-tab split-pane terminal with command history
export const TerminalPanel = forwardRef<TerminalPanelHandle, TerminalPanelProps>(
  function TerminalPanel(
    { panelState, onChangePanelState, onTabCountChange, shortcuts }: TerminalPanelProps,
    ref
  ) {
    const [tabs, setTabs] = useState<TerminalTab[]>([])
    const [activeId, setActiveId] = useState<string | null>(null)
    const [showHistory, setShowHistory] = useState(false)
    const [commandHistory, setCommandHistory] = useState<Map<string, CmdEntry[]>>(new Map())
    const [historyError, setHistoryError] = useState<string | null>(null)

    // Live instances keyed by pane ID (tab.id OR tab.splitPaneId)
    const instances = useRef<Map<string, TerminalInstance>>(new Map())
    const containerRefs = useRef<Map<string, HTMLDivElement>>(new Map())
    const disposedRef = useRef(false)
    const saveTimersRef = useRef(new Map<string, ReturnType<typeof setTimeout>>())

    // Command history per pane: map of pane ID → sorted CmdEntry[]
    const commandHistoryRef = useRef<Map<string, CmdEntry[]>>(new Map())
    // Canonical history per daemon key. Split panes share this list so one pane cannot overwrite another.
    const keyHistoryRef = useRef<Map<string, CmdEntry[]>>(new Map())
    // Maps pane ID → daemon storage key for history persistence
    const paneToKeyRef = useRef<Map<string, string>>(new Map())

    // Stale-closure-safe refs
    const shortcutsRef = useRef(shortcuts)
    const tabsRef = useRef(tabs)
    const activeIdRef = useRef(activeId)
    const focusedPaneIdRef = useRef<string | null>(null)

    // Action refs used by keyboard handler inside xterm (avoids stale closures in attachCustomKeyEventHandler)
    const actionsRef = useRef({ splitPane: () => {}, duplicateTab: () => {}, newTab: () => {} })

    const panelBodyRef = useRef<HTMLDivElement>(null)

    const releasePaneHistory = useCallback((paneId: string) => {
      const key = paneToKeyRef.current.get(paneId)
      paneToKeyRef.current.delete(paneId)
      if (key && !Array.from(paneToKeyRef.current.values()).includes(key)) {
        keyHistoryRef.current.delete(key)
      }
    }, [])

    // Keep refs in sync
    useEffect(() => {
      shortcutsRef.current = shortcuts
    }, [shortcuts])
    useEffect(() => {
      tabsRef.current = tabs
    }, [tabs])
    useEffect(() => {
      activeIdRef.current = activeId
    }, [activeId])

    // @group BusinessLogic > Terminal : Add a new tab and restore persisted command history
    const addTab = useCallback(
      (cwd?: string, name?: string) => {
        if (tabsRef.current.length >= MAX_TERMINAL_TABS) {
          setHistoryError(`最多同时打开 ${MAX_TERMINAL_TABS} 个终端标签，请先关闭一个标签`)
          return
        }
        const id = crypto.randomUUID()
        const resolvedCwd = cwd ?? ''
        const label = name
          ? name
          : resolvedCwd
            ? (resolvedCwd.split(/[\\/]/).filter(Boolean).pop() ?? '终端')
            : '终端'
        // Stable, bounded key includes cwd so same-named processes do not share history.
        const processKey = terminalHistoryKey(name, resolvedCwd)

        setTabs(prev => {
          const next = [...prev, { id, title: label, cwd: resolvedCwd, processKey }]
          onTabCountChange(next.length)
          return next
        })
        setActiveId(id)

        // Register key and restore saved history from daemon storage
        if (processKey) {
          paneToKeyRef.current.set(id, processKey)
          const cached = keyHistoryRef.current.get(processKey)
          if (cached) {
            commandHistoryRef.current.set(id, [...cached])
            setCommandHistory(new Map(commandHistoryRef.current))
            return
          }
          loadPersistedHistory(processKey)
            .then(saved => {
              if (disposedRef.current || paneToKeyRef.current.get(id) !== processKey) return
              setHistoryError(null)
              if (saved.length > 0) {
                const merged = mergeTerminalHistory(
                  keyHistoryRef.current.get(processKey) ?? [],
                  saved
                )
                keyHistoryRef.current.set(processKey, merged)
                for (const [paneId, key] of paneToKeyRef.current) {
                  if (key === processKey) commandHistoryRef.current.set(paneId, [...merged])
                }
                setCommandHistory(new Map(commandHistoryRef.current))
              }
            })
            .catch(error => {
              if (disposedRef.current || paneToKeyRef.current.get(id) !== processKey) return
              setHistoryError(error instanceof Error ? error.message : '读取命令历史失败')
            })
        }
      },
      [onTabCountChange]
    )

    // @group BusinessLogic > Terminal : Split the active tab's primary pane
    const splitActivePane = useCallback(() => {
      const aid = activeIdRef.current
      if (!aid) return
      const tab = tabsRef.current.find(candidate => candidate.id === aid)
      if (!tab || tab.splitPaneId) return
      const splitId = crypto.randomUUID()
      if (tab.processKey) {
        paneToKeyRef.current.set(splitId, tab.processKey)
        const existing =
          keyHistoryRef.current.get(tab.processKey) ?? commandHistoryRef.current.get(tab.id)
        if (existing?.length) commandHistoryRef.current.set(splitId, [...existing])
        setCommandHistory(new Map(commandHistoryRef.current))
      }
      setTabs(prev =>
        prev.map(candidate =>
          candidate.id === aid ? { ...candidate, splitPaneId: splitId } : candidate
        )
      )
    }, [])

    // @group BusinessLogic > Terminal : Duplicate the active tab (same cwd + title, new PTY)
    const duplicateActiveTab = useCallback(() => {
      const tab = tabsRef.current.find(t => t.id === activeIdRef.current)
      if (tab) addTab(tab.cwd, tab.title)
    }, [addTab])

    // Update actionsRef so keyboard handlers always call the latest functions
    useEffect(() => {
      actionsRef.current = {
        splitPane: splitActivePane,
        duplicateTab: duplicateActiveTab,
        newTab: () => addTab(),
      }
    }, [splitActivePane, duplicateActiveTab, addTab])

    // @group BusinessLogic > Terminal : Expose openTab to parent via ref
    useImperativeHandle(ref, () => ({ openTab: addTab }), [addTab])

    // @group BusinessLogic > Terminal : Close a tab — dispose all panes
    const closeTab = useCallback(
      (id: string) => {
        const tab = tabsRef.current.find(candidate => candidate.id === id)
        if (tab) {
          ;[tab.id, ...(tab.splitPaneId ? [tab.splitPaneId] : [])].forEach(pid => {
            const instance = instances.current.get(pid)
            instance?.connectAbort.abort()
            instance?.ws?.close()
            instance?.term.dispose()
            instances.current.delete(pid)
            containerRefs.current.delete(pid)
            commandHistoryRef.current.delete(pid)
            releasePaneHistory(pid)
          })
          setCommandHistory(new Map(commandHistoryRef.current))
        }
        setTabs(prev => {
          const next = prev.filter(t => t.id !== id)
          onTabCountChange(next.length)
          return next
        })
        setActiveId(prev => {
          if (prev !== id) return prev
          const remaining = tabsRef.current.filter(t => t.id !== id)
          return remaining.length > 0 ? remaining[remaining.length - 1].id : null
        })
      },
      [onTabCountChange, releasePaneHistory]
    )

    // @group BusinessLogic > Terminal : Close only the split pane of a tab
    const closeSplitPane = useCallback(
      (tabId: string) => {
        const tab = tabsRef.current.find(candidate => candidate.id === tabId)
        if (tab?.splitPaneId) {
          const sid = tab.splitPaneId
          const instance = instances.current.get(sid)
          instance?.connectAbort.abort()
          instance?.ws?.close()
          instance?.term.dispose()
          instances.current.delete(sid)
          containerRefs.current.delete(sid)
          commandHistoryRef.current.delete(sid)
          releasePaneHistory(sid)
          setCommandHistory(new Map(commandHistoryRef.current))
        }
        setTabs(prev =>
          prev.map(candidate =>
            candidate.id === tabId ? { ...candidate, splitPaneId: undefined } : candidate
          )
        )
      },
      [releasePaneHistory]
    )

    // @group BusinessLogic > Terminal : Initialize xterm + WebSocket for a pane container div
    const initTerminal = useCallback(async (id: string, el: HTMLDivElement, cwd: string) => {
      if (instances.current.has(id)) return // already initialized

      const term = new Terminal({
        theme: {
          background: '#0d0d0d',
          foreground: '#e8e8e8',
          cursor: '#e8e8e8',
          black: '#000000',
          red: '#f87171',
          green: '#4ade80',
          yellow: '#fbbf24',
          blue: '#60a5fa',
          magenta: '#c084fc',
          cyan: '#22d3ee',
          white: '#e5e7eb',
          brightBlack: '#6b7280',
          brightRed: '#fca5a5',
          brightGreen: '#86efac',
          brightYellow: '#fde68a',
          brightBlue: '#93c5fd',
          brightMagenta: '#d8b4fe',
          brightCyan: '#67e8f9',
          brightWhite: '#f9fafb',
        },
        fontFamily: '"Cascadia Code", "Fira Code", "JetBrains Mono", Consolas, monospace',
        fontSize: 13,
        lineHeight: 1.4,
        cursorBlink: true,
        scrollback: 5000,
      })

      const fitAddon = new FitAddon()
      term.loadAddon(fitAddon)
      term.open(el)
      fitAddon.fit()

      const connectAbort = new AbortController()
      const instance: TerminalInstance = {
        term,
        fitAddon,
        ws: null,
        connected: false,
        connectAbort,
      }
      instances.current.set(id, instance)

      // Connect WebSocket
      let wsUrl: string | null = null
      let retryDelayMs = 1_000
      let connectAttempts = 0
      const connectDeadline = Date.now() + MAX_TERMINAL_CONNECT_MS
      while (
        !disposedRef.current &&
        !connectAbort.signal.aborted &&
        instances.current.get(id) === instance &&
        wsUrl === null &&
        connectAttempts < MAX_TERMINAL_CONNECT_ATTEMPTS &&
        Date.now() < connectDeadline
      ) {
        connectAttempts += 1
        try {
          wsUrl = await buildTerminalWsUrl(cwd, term.cols, term.rows, connectAbort.signal)
        } catch (error) {
          if (connectAbort.signal.aborted) return
          term.writeln(
            `\r\n\x1b[31m[连接错误] ${error instanceof Error ? error.message : '无法创建安全流凭据'}，${Math.ceil(retryDelayMs / 1_000)} 秒后重试\x1b[0m`
          )
          try {
            await waitForAbortableDelay(retryDelayMs, connectAbort.signal)
          } catch {
            return
          }
          retryDelayMs = Math.min(retryDelayMs * 2, 30_000)
        }
      }
      if (
        disposedRef.current ||
        connectAbort.signal.aborted ||
        instances.current.get(id) !== instance
      )
        return
      if (wsUrl === null) {
        term.writeln(
          `\r\n\x1b[31m[连接已停止] ${connectAttempts} 次尝试后仍无法创建安全流凭据。请关闭标签后重试。\x1b[0m`
        )
        return
      }
      const ws = new WebSocket(wsUrl)
      ws.binaryType = 'arraybuffer'
      instance.ws = ws

      ws.onopen = () => {
        instance.connected = true
      }
      ws.onerror = () => {
        term.writeln('\r\n\x1b[31m[连接错误]\x1b[0m')
      }
      ws.onclose = () => {
        if (instance.connected) term.writeln('\r\n\x1b[33m[已断开连接]\x1b[0m')
        instance.connected = false
      }
      ws.onmessage = (e: MessageEvent) => {
        if (e.data instanceof ArrayBuffer) {
          term.write(new Uint8Array(e.data))
        } else if (typeof e.data === 'string') {
          try {
            const msg = JSON.parse(e.data)
            if (msg.type === 'error') term.writeln(`\r\n\x1b[31m[错误] ${msg.message}\x1b[0m`)
            else if (msg.type === 'exit')
              term.writeln(`\r\n\x1b[33m[进程已退出，退出码 ${msg.code}]\x1b[0m`)
          } catch {
            term.write(e.data)
          }
        }
      }

      // @group BusinessLogic > Terminal > Input : Track input buffer → command history, then forward to PTY
      // Strips ANSI/VT escape sequences (PSReadLine sends them for syntax highlighting/cursor moves)
      let inputBuf = ''
      let inEscape = false // true while inside an ESC [...] CSI sequence
      let inOsc = false // true while inside an OSC ESC ] ... BEL/ST sequence
      term.onData((data: string) => {
        for (const ch of data) {
          const code = ch.charCodeAt(0)
          if (ch === '\x1b') {
            inEscape = true
            inOsc = false
            continue
          }
          if (inEscape) {
            if (ch === ']') {
              inOsc = true
              inEscape = false
              continue
            } // OSC start
            // End CSI on any letter or ~ (e.g. ESC[1m, ESC[A, ESC[?25h)
            if ((ch >= 'A' && ch <= 'Z') || (ch >= 'a' && ch <= 'z') || ch === '~') inEscape = false
            continue
          }
          if (inOsc) {
            if (ch === '\x07' || ch === '\x9c') inOsc = false // BEL or ST ends OSC
            continue
          }
          if (ch === '\r') {
            const cmd = inputBuf.trim()
            if (cmd) {
              const key = paneToKeyRef.current.get(id)
              const hist = [
                ...(key
                  ? (keyHistoryRef.current.get(key) ?? commandHistoryRef.current.get(id) ?? [])
                  : (commandHistoryRef.current.get(id) ?? [])),
              ]
              const ex = hist.find(h => h.cmd === cmd)
              if (ex) {
                ex.count++
              } else {
                hist.unshift({ cmd, count: 1 })
              }
              const trimmed = hist.slice(0, 150)
              if (key) {
                keyHistoryRef.current.set(key, trimmed)
                for (const [paneId, paneKey] of paneToKeyRef.current) {
                  if (paneKey === key) commandHistoryRef.current.set(paneId, [...trimmed])
                }
              } else {
                commandHistoryRef.current.set(id, trimmed)
              }
              setCommandHistory(new Map(commandHistoryRef.current))
              if (key) {
                savePersistedHistory(saveTimersRef.current, key, trimmed, message =>
                  setHistoryError(message)
                )
              }
            }
            inputBuf = ''
          } else if (ch === '\x7f' || ch === '\b') {
            inputBuf = inputBuf.slice(0, -1)
          } else if (code >= 32) {
            inputBuf += ch
          }
        }
        if (ws.readyState === WebSocket.OPEN) {
          ws.send(JSON.stringify({ type: 'input', data }))
        }
      })

      // Track which pane has focus (for history paste target)
      term.textarea?.addEventListener('focus', () => {
        focusedPaneIdRef.current = id
      })

      // Forward resize events
      term.onResize(({ cols, rows }: { cols: number; rows: number }) => {
        if (ws.readyState === WebSocket.OPEN) {
          ws.send(JSON.stringify({ type: 'resize', cols, rows }))
        }
      })

      // @group BusinessLogic > Terminal > Shortcuts : Intercept keyboard shortcuts before xterm processes them
      term.attachCustomKeyEventHandler((e: KeyboardEvent) => {
        if (e.type !== 'keydown') return true
        const s = shortcutsRef.current
        if (matchesShortcut(e, s.splitPane)) {
          e.preventDefault()
          actionsRef.current.splitPane()
          return false
        }
        if (matchesShortcut(e, s.duplicateTab)) {
          e.preventDefault()
          actionsRef.current.duplicateTab()
          return false
        }
        if (matchesShortcut(e, s.newTab)) {
          e.preventDefault()
          actionsRef.current.newTab()
          return false
        }
        return true
      })
    }, [])

    // @group BusinessLogic > Terminal : Register container div and initialize terminal
    const setContainerRef = useCallback(
      (id: string, cwd: string) => (el: HTMLDivElement | null) => {
        if (el) {
          containerRefs.current.set(id, el)
          initTerminal(id, el, cwd)
        } else {
          containerRefs.current.delete(id)
        }
      },
      [initTerminal]
    )

    // @group BusinessLogic > Terminal : Fit all panes of a tab
    const fitTab = useCallback((tab: TerminalTab) => {
      const paneIds = [tab.id, ...(tab.splitPaneId ? [tab.splitPaneId] : [])]
      paneIds.forEach(pid => {
        const inst = instances.current.get(pid)
        if (inst) {
          inst.fitAddon.fit()
          inst.term.scrollToBottom()
        }
      })
    }, [])

    // @group BusinessLogic > Terminal : Re-fit on active tab or panel state change
    useEffect(() => {
      if (!activeId || panelState === 'hidden') return
      const tab = tabs.find(t => t.id === activeId)
      if (!tab) return
      const t = setTimeout(() => {
        requestAnimationFrame(() => fitTab(tab))
      }, 30)
      return () => clearTimeout(t)
    }, [activeId, panelState, tabs, fitTab])

    // @group BusinessLogic > Terminal : ResizeObserver — re-fit whenever panel body size changes
    useEffect(() => {
      const el = panelBodyRef.current
      if (!el) return
      const ro = new ResizeObserver(() => {
        const tab = tabsRef.current.find(t => t.id === activeIdRef.current)
        if (!tab) return
        requestAnimationFrame(() => fitTab(tab))
      })
      ro.observe(el)
      return () => ro.disconnect()
    }, [fitTab])

    // @group BusinessLogic > Terminal : Auto-open first tab when panel first becomes visible
    useEffect(() => {
      if (panelState === 'hidden' || tabs.length !== 0) return
      let active = true
      queueMicrotask(() => {
        if (active) addTab()
      })
      return () => {
        active = false
      }
    }, [panelState, tabs.length, addTab])

    // @group BusinessLogic > Terminal : Cleanup all instances on unmount
    useEffect(() => {
      disposedRef.current = false
      const activeInstances = instances.current
      const activeContainers = containerRefs.current
      const activeHistory = commandHistoryRef.current
      const activeKeyHistory = keyHistoryRef.current
      const activePaneKeys = paneToKeyRef.current
      const activeSaveTimers = saveTimersRef.current
      return () => {
        disposedRef.current = true
        activeSaveTimers.forEach(clearTimeout)
        activeSaveTimers.clear()
        activeInstances.forEach(inst => {
          inst.connectAbort.abort()
          inst.ws?.close()
          inst.term.dispose()
        })
        activeInstances.clear()
        activeContainers.clear()
        activeHistory.clear()
        activeKeyHistory.clear()
        activePaneKeys.clear()
      }
    }, [])

    // @group BusinessLogic > Terminal > History : Merge history of all panes in the active tab, sorted by count
    const activeHistory = (() => {
      const tab = tabs.find(t => t.id === activeId)
      if (!tab) return []
      const merged = new Map<string, number>()
      ;[tab.id, ...(tab.splitPaneId ? [tab.splitPaneId] : [])].forEach(pid => {
        commandHistory.get(pid)?.forEach(({ cmd, count }) => {
          merged.set(cmd, (merged.get(cmd) ?? 0) + count)
        })
      })
      return [...merged.entries()]
        .map(([cmd, count]) => ({ cmd, count }))
        .sort((a, b) => b.count - a.count)
    })()

    // @group BusinessLogic > Terminal > History : Paste a command into the focused pane (no execute)
    function pasteToTerminal(cmd: string) {
      const targetId = focusedPaneIdRef.current ?? activeId
      if (!targetId) return
      const inst = instances.current.get(targetId)
      if (inst?.ws?.readyState === WebSocket.OPEN) {
        inst.ws.send(JSON.stringify({ type: 'input', data: cmd }))
        inst.term.focus()
      }
    }

    // @group Styles : Panel — slides up/down via transform for animation
    const hidden = panelState === 'hidden'
    const isMax = panelState === 'maximized'

    const panelStyle: React.CSSProperties = {
      position: 'fixed',
      left: 0,
      right: 0,
      bottom: 22,
      zIndex: isMax ? 600 : 500,
      display: 'flex',
      flexDirection: 'column',
      background: '#0d0d0d',
      transform: hidden ? 'translateY(calc(100% + 22px))' : 'translateY(0)',
      transition: 'transform 0.28s cubic-bezier(0.4, 0, 0.2, 1)',
      willChange: 'transform',
      pointerEvents: hidden ? 'none' : 'auto',
      ...(isMax
        ? { top: 0, borderTop: '1px solid var(--color-border)' }
        : {
            height: '45vh',
            minHeight: 200,
            borderTop: '1px solid #333',
            boxShadow: '0 -4px 20px rgba(0,0,0,0.5)',
          }),
    }

    const headerStyle: React.CSSProperties = {
      display: 'flex',
      alignItems: 'center',
      height: 34,
      minHeight: 34,
      background: '#161616',
      borderBottom: '1px solid #2a2a2a',
      userSelect: 'none',
      flexShrink: 0,
    }

    const tabStyle = (active: boolean): React.CSSProperties => ({
      display: 'flex',
      alignItems: 'center',
      gap: 5,
      padding: '0 10px 0 12px',
      height: '100%',
      fontSize: 12,
      cursor: 'pointer',
      background: active ? '#0d0d0d' : 'transparent',
      color: active ? '#e8e8e8' : '#888',
      borderRight: '1px solid #2a2a2a',
      borderTop: active ? '1px solid var(--color-primary)' : '1px solid transparent',
      whiteSpace: 'nowrap',
      flexShrink: 0,
      transition: 'color 0.1s',
    })

    const iconBtnStyle: React.CSSProperties = {
      display: 'flex',
      alignItems: 'center',
      justifyContent: 'center',
      width: 26,
      height: 26,
      flexShrink: 0,
      background: 'transparent',
      border: 'none',
      color: '#888',
      cursor: 'pointer',
      borderRadius: 4,
      padding: 0,
    }

    return (
      <div style={panelStyle} aria-hidden={hidden} inert={hidden}>
        {/* Header: icon + tabs + action buttons + window controls */}
        <div style={headerStyle}>
          {/* Terminal label */}
          <div
            role="tablist"
            aria-label="终端标签页"
            style={{
              display: 'flex',
              alignItems: 'center',
              gap: 6,
              padding: '0 10px',
              color: '#555',
              flexShrink: 0,
            }}
          >
            <SquareTerminal size={13} />
            <span
              style={{
                fontSize: 11,
                fontWeight: 600,
                letterSpacing: '0.05em',
                textTransform: 'uppercase',
                opacity: 0.7,
              }}
            >
              终端
            </span>
          </div>

          {/* Tab list */}
          <div
            style={{
              display: 'flex',
              alignItems: 'stretch',
              flex: 1,
              overflow: 'hidden',
              height: '100%',
            }}
          >
            {tabs.map(tab => (
              <div
                key={tab.id}
                role="tab"
                tabIndex={0}
                aria-selected={tab.id === activeId}
                style={tabStyle(tab.id === activeId)}
                onClick={() => setActiveId(tab.id)}
                onKeyDown={event => {
                  if (event.key === 'Enter' || event.key === ' ') {
                    event.preventDefault()
                    setActiveId(tab.id)
                  }
                }}
              >
                <SquareTerminal size={11} style={{ opacity: 0.7, flexShrink: 0 }} />
                <span style={{ maxWidth: 120, overflow: 'hidden', textOverflow: 'ellipsis' }}>
                  {tab.title}
                </span>
                {tab.splitPaneId && (
                  <span
                    title="拆分窗格已激活"
                    style={{ display: 'flex', flexShrink: 0, marginLeft: 1, opacity: 0.45 }}
                  >
                    <SquareSplitHorizontal size={9} />
                  </span>
                )}
                <button
                  style={{ ...iconBtnStyle, width: 16, height: 16, marginLeft: 2, color: '#666' }}
                  onClick={e => {
                    e.stopPropagation()
                    closeTab(tab.id)
                  }}
                  title="关闭标签页"
                  onMouseEnter={e => {
                    ;(e.currentTarget as HTMLElement).style.color = '#f87171'
                  }}
                  onMouseLeave={e => {
                    ;(e.currentTarget as HTMLElement).style.color = '#666'
                  }}
                >
                  <X size={10} />
                </button>
              </div>
            ))}
          </div>

          {/* New tab */}
          <button
            style={{ ...iconBtnStyle, marginLeft: 4, flexShrink: 0 }}
            onClick={() => addTab()}
            title={`新建终端（${shortcuts.newTab}）`}
            onMouseEnter={e => {
              ;(e.currentTarget as HTMLElement).style.color = '#e8e8e8'
            }}
            onMouseLeave={e => {
              ;(e.currentTarget as HTMLElement).style.color = '#888'
            }}
          >
            <Plus size={13} />
          </button>

          {/* Split pane */}
          <button
            style={{ ...iconBtnStyle, flexShrink: 0 }}
            onClick={splitActivePane}
            title={`拆分窗格（${shortcuts.splitPane}）`}
            onMouseEnter={e => {
              ;(e.currentTarget as HTMLElement).style.color = '#e8e8e8'
            }}
            onMouseLeave={e => {
              ;(e.currentTarget as HTMLElement).style.color = '#888'
            }}
          >
            <SquareSplitHorizontal size={13} />
          </button>

          {/* Command history toggle */}
          <button
            style={{
              ...iconBtnStyle,
              flexShrink: 0,
              color: showHistory ? 'var(--color-primary)' : '#888',
            }}
            onClick={() => setShowHistory(s => !s)}
            title="命令历史"
            onMouseEnter={e => {
              if (!showHistory) (e.currentTarget as HTMLElement).style.color = '#e8e8e8'
            }}
            onMouseLeave={e => {
              if (!showHistory)
                (e.currentTarget as HTMLElement).style.color = showHistory
                  ? 'var(--color-primary)'
                  : '#888'
            }}
          >
            <History size={13} />
          </button>

          <div style={{ flex: 1 }} />

          {/* Minimize */}
          <button
            style={{ ...iconBtnStyle, marginRight: 2 }}
            onClick={() => onChangePanelState('hidden')}
            title="最小化"
            onMouseEnter={e => {
              ;(e.currentTarget as HTMLElement).style.color = '#e8e8e8'
            }}
            onMouseLeave={e => {
              ;(e.currentTarget as HTMLElement).style.color = '#888'
            }}
          >
            <ChevronDown size={14} />
          </button>

          {/* Maximize / Restore */}
          <button
            style={{ ...iconBtnStyle, marginRight: 6 }}
            onClick={() => onChangePanelState(isMax ? 'normal' : 'maximized')}
            title={isMax ? '还原' : '最大化'}
            onMouseEnter={e => {
              ;(e.currentTarget as HTMLElement).style.color = '#e8e8e8'
            }}
            onMouseLeave={e => {
              ;(e.currentTarget as HTMLElement).style.color = '#888'
            }}
          >
            {isMax ? <Minimize2 size={13} /> : <Maximize2 size={13} />}
          </button>
        </div>

        {/* Body: terminal pane area + optional history sidebar */}
        <div style={{ flex: 1, overflow: 'hidden', display: 'flex' }}>
          {/* Terminal pane area */}
          <div ref={panelBodyRef} style={{ flex: 1, overflow: 'hidden', position: 'relative' }}>
            {tabs.map(tab => (
              <div
                key={tab.id}
                style={{
                  position: 'absolute',
                  inset: 0,
                  display: tab.id === activeId ? 'flex' : 'none',
                }}
              >
                {/* Primary pane */}
                <div style={{ flex: 1, position: 'relative', minWidth: 0 }}>
                  <div
                    ref={setContainerRef(tab.id, tab.cwd)}
                    style={{
                      position: 'absolute',
                      inset: 0,
                      padding: '6px 8px',
                      boxSizing: 'border-box',
                    }}
                  />
                </div>

                {/* Split pane */}
                {tab.splitPaneId && (
                  <>
                    <div style={{ width: 2, background: '#1e1e1e', flexShrink: 0 }} />
                    <div style={{ flex: 1, position: 'relative', minWidth: 0 }}>
                      <button
                        title="关闭拆分窗格"
                        onClick={() => closeSplitPane(tab.id)}
                        style={{
                          position: 'absolute',
                          top: 4,
                          right: 6,
                          zIndex: 10,
                          width: 18,
                          height: 18,
                          padding: 0,
                          border: 'none',
                          borderRadius: 3,
                          background: 'rgba(255,255,255,0.06)',
                          color: '#555',
                          cursor: 'pointer',
                          display: 'flex',
                          alignItems: 'center',
                          justifyContent: 'center',
                        }}
                        onMouseEnter={e => {
                          ;(e.currentTarget as HTMLElement).style.color = '#f87171'
                        }}
                        onMouseLeave={e => {
                          ;(e.currentTarget as HTMLElement).style.color = '#555'
                        }}
                      >
                        <X size={10} />
                      </button>
                      <div
                        ref={setContainerRef(tab.splitPaneId, tab.cwd)}
                        style={{
                          position: 'absolute',
                          inset: 0,
                          padding: '6px 8px',
                          boxSizing: 'border-box',
                        }}
                      />
                    </div>
                  </>
                )}
              </div>
            ))}

            {tabs.length === 0 && (
              <div
                style={{
                  display: 'flex',
                  alignItems: 'center',
                  justifyContent: 'center',
                  height: '100%',
                  color: '#555',
                  fontSize: 13,
                  gap: 8,
                }}
              >
                <SquareTerminal size={18} />
                <span>点击 + 打开终端</span>
              </div>
            )}
          </div>

          {/* Command history sidebar */}
          {showHistory && (
            <div
              style={{
                width: 210,
                flexShrink: 0,
                borderLeft: '1px solid #2a2a2a',
                background: '#0b0b0b',
                display: 'flex',
                flexDirection: 'column',
                overflow: 'hidden',
              }}
            >
              <div
                style={{
                  padding: '6px 10px',
                  fontSize: 10,
                  fontWeight: 700,
                  letterSpacing: '0.07em',
                  textTransform: 'uppercase',
                  color: '#555',
                  borderBottom: '1px solid #1e1e1e',
                  display: 'flex',
                  alignItems: 'center',
                  justifyContent: 'space-between',
                  flexShrink: 0,
                }}
              >
                <span>历史记录</span>
                <span style={{ color: '#3a3a3a', fontWeight: 400, fontSize: 11 }}>
                  {activeHistory.length}
                </span>
              </div>
              {historyError && (
                <div
                  role="alert"
                  style={{
                    padding: '6px 10px',
                    fontSize: 10,
                    color: '#f87171',
                    borderBottom: '1px solid #1e1e1e',
                  }}
                >
                  {historyError}
                </div>
              )}
              <div style={{ flex: 1, overflowY: 'auto' }}>
                {activeHistory.length === 0 ? (
                  <div
                    style={{
                      padding: '12px 10px',
                      fontSize: 11,
                      color: '#3a3a3a',
                      fontStyle: 'italic',
                    }}
                  >
                    暂无命令记录，请输入内容！
                  </div>
                ) : (
                  activeHistory.map(({ cmd, count }) => (
                    <div
                      key={cmd}
                      role="button"
                      tabIndex={0}
                      title={`${cmd}\n点击粘贴`}
                      onClick={() => pasteToTerminal(cmd)}
                      onKeyDown={event => {
                        if (event.key === 'Enter' || event.key === ' ') {
                          event.preventDefault()
                          pasteToTerminal(cmd)
                        }
                      }}
                      style={{
                        display: 'flex',
                        alignItems: 'center',
                        gap: 6,
                        padding: '5px 10px',
                        cursor: 'pointer',
                        borderBottom: '1px solid #131313',
                      }}
                      onMouseEnter={e => {
                        ;(e.currentTarget as HTMLElement).style.background = '#181818'
                      }}
                      onMouseLeave={e => {
                        ;(e.currentTarget as HTMLElement).style.background = 'transparent'
                      }}
                    >
                      <code
                        style={{
                          flex: 1,
                          fontSize: 11,
                          color: '#aaa',
                          fontFamily: '"Cascadia Code", "Fira Code", Consolas, monospace',
                          overflow: 'hidden',
                          textOverflow: 'ellipsis',
                          whiteSpace: 'nowrap',
                        }}
                      >
                        {cmd}
                      </code>
                      {count > 1 && (
                        <span
                          style={{
                            fontSize: 9,
                            fontWeight: 700,
                            padding: '1px 5px',
                            borderRadius: 10,
                            background: 'rgba(96,165,250,0.1)',
                            color: '#4a90d9',
                            flexShrink: 0,
                            minWidth: 18,
                            textAlign: 'center' as const,
                          }}
                        >
                          {count}
                        </span>
                      )}
                    </div>
                  ))
                )}
              </div>
            </div>
          )}
        </div>
      </div>
    )
  }
)

// @group Exports : Status bar pill — active terminal count badge, click to show/hide panel
export function TerminalStatusBarBtn({
  panelState,
  onToggle,
  tabCount,
}: {
  panelState: TerminalPanelState
  onToggle: () => void
  tabCount: number
}) {
  const active = panelState !== 'hidden'
  const style: React.CSSProperties = {
    display: 'flex',
    alignItems: 'center',
    gap: 4,
    padding: '0 9px',
    height: '100%',
    cursor: 'pointer',
    background: active
      ? 'color-mix(in srgb, var(--color-foreground) 10%, transparent)'
      : 'transparent',
    border: 'none',
    borderLeft: '1px solid var(--color-border)',
    color: active ? 'var(--color-foreground)' : 'var(--color-muted-foreground)',
    fontFamily: 'inherit',
    fontSize: 11,
    fontWeight: 500,
    position: 'relative',
  }
  return (
    <button
      style={style}
      onClick={onToggle}
      title={active ? '隐藏终端' : '显示终端'}
      onMouseEnter={e => {
        ;(e.currentTarget as HTMLElement).style.background =
          'color-mix(in srgb, var(--color-foreground) 10%, transparent)'
      }}
      onMouseLeave={e => {
        ;(e.currentTarget as HTMLElement).style.background = active
          ? 'color-mix(in srgb, var(--color-foreground) 10%, transparent)'
          : 'transparent'
      }}
    >
      <SquareTerminal size={12} />
      {tabCount > 0 && (
        <span
          style={{
            position: 'absolute',
            top: 2,
            right: 4,
            minWidth: 13,
            height: 13,
            borderRadius: 7,
            background: 'var(--color-primary)',
            color: '#000',
            fontSize: 8,
            fontWeight: 700,
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            padding: '0 2px',
            lineHeight: 1,
          }}
        >
          {tabCount > 9 ? '9+' : tabCount}
        </span>
      )}
    </button>
  )
}
