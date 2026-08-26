// @group BusinessLogic : Sequential polling that never overlaps requests or commits stale results

import { useCallback, useEffect, useRef } from 'react'

type PollTask = (isCurrent: () => boolean, signal: AbortSignal) => Promise<void>

interface PollOptions {
  intervalMs: number
  enabled?: boolean
  refreshKey?: string | number
}

export function useSingleFlightPoll(
  task: PollTask,
  { intervalMs, enabled = true, refreshKey = '' }: PollOptions
) {
  const taskRef = useRef(task)
  const activeRef = useRef(false)
  const generationRef = useRef(0)
  const inFlightRef = useRef<Promise<void> | null>(null)
  const abortRef = useRef<AbortController | null>(null)
  const consecutiveFailuresRef = useRef(0)

  useEffect(() => {
    taskRef.current = task
  }, [task])

  const reload = useCallback(async () => {
    if (!activeRef.current) return
    if (inFlightRef.current) return inFlightRef.current

    const generation = generationRef.current
    const controller = new AbortController()
    abortRef.current = controller
    const isCurrent = () =>
      activeRef.current && generationRef.current === generation && !controller.signal.aborted

    const request = taskRef
      .current(isCurrent, controller.signal)
      .then(() => {
        if (isCurrent()) consecutiveFailuresRef.current = 0
      })
      .catch(error => {
        if (isCurrent() && !(error instanceof DOMException && error.name === 'AbortError')) {
          consecutiveFailuresRef.current = Math.min(consecutiveFailuresRef.current + 1, 4)
        }
      })
      .finally(() => {
        if (inFlightRef.current === request) inFlightRef.current = null
        if (abortRef.current === controller) abortRef.current = null
      })
    inFlightRef.current = request
    return request
  }, [])

  useEffect(() => {
    activeRef.current = true
    const generation = ++generationRef.current
    consecutiveFailuresRef.current = 0
    let timer: ReturnType<typeof setTimeout> | null = null
    const safeIntervalMs =
      Number.isFinite(intervalMs) && intervalMs >= 100 ? Math.min(intervalMs, 5 * 60_000) : 1_000

    const tick = async () => {
      await reload()
      if (activeRef.current && generationRef.current === generation && enabled) {
        const backoff = 2 ** consecutiveFailuresRef.current
        timer = setTimeout(() => void tick(), Math.min(safeIntervalMs * backoff, 5 * 60_000))
      }
    }
    if (enabled) void tick()

    return () => {
      activeRef.current = false
      generationRef.current += 1
      abortRef.current?.abort()
      abortRef.current = null
      if (timer) clearTimeout(timer)
    }
  }, [enabled, intervalMs, refreshKey, reload])

  return reload
}
