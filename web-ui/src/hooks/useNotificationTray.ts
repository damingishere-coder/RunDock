// @group BusinessLogic : In-app notification tray — detects process state transitions and surfaces them as activity events

import { useCallback, useEffect, useRef, useState } from 'react'
import type { ProcessInfo, ProcessStatus } from '@/types'

// @group Types > AppNotification : A single in-app activity event
export type NotifEvent = 'crash' | 'restart' | 'started' | 'stopped'

export interface AppNotification {
  id: string
  processId: string
  processName: string
  namespace: string
  event: NotifEvent
  detail: string // e.g. "Exit code: 1", "PID: 12345", "Auto-restarted ×3"
  timestamp: Date
  read: boolean
}

// @group Utilities > eventConfig : Visual config per event type
export const eventConfig: Record<NotifEvent, { color: string; label: string }> = {
  crash: { color: 'var(--color-status-crashed)', label: '已崩溃' },
  restart: { color: 'var(--color-status-starting)', label: '已重启' },
  started: { color: 'var(--color-status-running)', label: '已启动' },
  stopped: { color: 'var(--color-status-stopped)', label: '已停止' },
}

// @group Utilities > isActive : True for statuses that mean "process is doing something"
const ACTIVE: ReadonlySet<ProcessStatus> = new Set(['running', 'watching'])
const INACTIVE: ReadonlySet<ProcessStatus> = new Set([
  'stopped',
  'crashed',
  'errored',
  'sleeping',
  'starting',
])

// @group Utilities > relativeTime : Human-readable relative timestamp
export function relativeTime(date: Date, now = Date.now()): string {
  const secs = Math.floor((now - date.getTime()) / 1000)
  if (secs < 5) return '刚刚'
  if (secs < 60) return `${secs}秒前`
  const mins = Math.floor(secs / 60)
  if (mins < 60) return `${mins}分钟前`
  const hrs = Math.floor(mins / 60)
  if (hrs < 24) return `${hrs}小时前`
  return `${Math.floor(hrs / 24)}天前`
}

// @group BusinessLogic > useNotificationTray : Main hook — consumes process list, emits AppNotifications on transitions
export function useNotificationTray(processes: ProcessInfo[]) {
  const [notifications, setNotifications] = useState<AppNotification[]>([])

  // @group BusinessLogic > PrevState : Tracks last-known status + restart_count per process
  type PrevState = { status: ProcessStatus; restart_count: number }
  const prevMapRef = useRef<Map<string, PrevState>>(new Map())
  const initializedRef = useRef(false)

  useEffect(() => {
    const prevMap = prevMapRef.current
    const newNotifs: AppNotification[] = []

    for (const p of processes) {
      const prev = prevMap.get(p.id)

      if (prev === undefined) {
        // First time we've seen this process — record state, no notification
        prevMap.set(p.id, { status: p.status, restart_count: p.restart_count })
        continue
      }

      const { status: prevStatus, restart_count: prevCount } = prev
      const curStatus = p.status
      const curCount = p.restart_count

      // Skip if nothing relevant changed
      if (prevStatus === curStatus && prevCount === curCount) continue

      let event: NotifEvent | null = null
      let detail = ''

      if (curStatus === 'errored' || curStatus === 'crashed') {
        // Crashed / hit max restarts
        event = 'crash'
        detail = p.last_exit_code != null ? `退出码：${p.last_exit_code}` : '进程出错'
      } else if (ACTIVE.has(prevStatus) && curStatus === 'stopped') {
        // Manually or naturally stopped
        event = 'stopped'
        detail = p.last_exit_code != null ? `退出码：${p.last_exit_code}` : '进程已停止'
      } else if (INACTIVE.has(prevStatus) && ACTIVE.has(curStatus)) {
        if (curCount > prevCount) {
          // Auto-restarted
          event = 'restart'
          detail = `自动重启 ×${curCount}`
        } else {
          // Freshly started / manually started
          event = 'started'
          detail = p.pid != null ? `PID：${p.pid}` : '进程已上线'
        }
      }

      if (event) {
        newNotifs.push({
          id: crypto.randomUUID(),
          processId: p.id,
          processName: p.name,
          namespace: p.namespace,
          event,
          detail,
          timestamp: new Date(),
          read: false,
        })
      }

      prevMap.set(p.id, { status: curStatus, restart_count: curCount })
    }

    // Remove stale entries for deleted processes
    const currentIds = new Set(processes.map(p => p.id))
    for (const id of prevMap.keys()) {
      if (!currentIds.has(id)) prevMap.delete(id)
    }

    // Skip adding notifications on the very first render (initial state population)
    if (!initializedRef.current) {
      initializedRef.current = true
      return
    }

    if (newNotifs.length > 0) {
      queueMicrotask(() => {
        setNotifications(prev => {
          // Newest first, cap at 50
          const combined = [...newNotifs, ...prev]
          return combined.slice(0, 50)
        })
      })
    }
  }, [processes])

  // @group BusinessLogic > Actions : Tray management actions
  const markAllRead = useCallback(() => {
    setNotifications(prev => prev.map(n => ({ ...n, read: true })))
  }, [])

  const clearAll = useCallback(() => {
    setNotifications([])
  }, [])

  const dismiss = useCallback((id: string) => {
    setNotifications(prev => prev.filter(n => n.id !== id))
  }, [])

  const unreadCount = notifications.filter(n => !n.read).length

  return { notifications, unreadCount, markAllRead, clearAll, dismiss }
}
