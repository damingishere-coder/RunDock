// @group BusinessLogic : Poll /api/v1/processes at a configurable interval

import { useCallback, useState } from 'react'
import { api } from '@/lib/api'
import type { ProcessInfo } from '@/types'
import { useSingleFlightPoll } from './useSingleFlightPoll'
import { isProcessInfo } from '@/lib/schemas'

// @group BusinessLogic > useProcesses : Polls the process list; interval and toggle driven by settings
export function useProcesses(autoRefresh = true, intervalMs = 3000) {
  const [processes, setProcesses] = useState<ProcessInfo[]>([])
  const [error, setError] = useState<string | null>(null)

  const load = useCallback(async (isCurrent: () => boolean, signal: AbortSignal) => {
    try {
      const data = await api.getProcesses({ signal })
      if (!isCurrent()) return
      if (!Array.isArray(data.processes) || !data.processes.every(isProcessInfo)) {
        throw new Error('进程列表响应格式无效')
      }
      setProcesses(data.processes)
      setError(null)
    } catch (loadError) {
      if (isCurrent()) {
        setError(loadError instanceof Error ? loadError.message : 'disconnected')
      }
      throw loadError
    }
  }, [])

  const reload = useSingleFlightPoll(load, { intervalMs, enabled: autoRefresh })

  return { processes, error, reload }
}
