// @group BusinessLogic : Fetch daemon health info at a configurable interval

import { useCallback, useState } from 'react'
import { api } from '@/lib/api'
import type { DaemonHealth } from '@/types'
import { isDaemonHealth } from '@/lib/schemas'
import { useSingleFlightPoll } from './useSingleFlightPoll'

// @group BusinessLogic > useDaemonHealth : Polls /system/health; interval driven by settings
export function useDaemonHealth(intervalMs = 5000) {
  const [health, setHealth] = useState<DaemonHealth | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [warning, setWarning] = useState<string | null>(null)

  const load = useCallback(async (isCurrent: () => boolean, signal: AbortSignal) => {
    try {
      const data = await api.getHealth({ signal })
      if (isCurrent()) {
        if (!isDaemonHealth(data)) {
          throw new Error('守护进程健康检查响应格式无效')
        }
        setHealth(data)
        setError(null)
        setWarning(
          data.status === 'degraded'
            ? (data.persistence_error ?? '后台状态持久化异常，请检查数据目录。')
            : null
        )
      }
    } catch (loadError) {
      if (isCurrent()) {
        setError(loadError instanceof Error ? loadError.message : '守护进程健康检查失败')
      }
      throw loadError
    }
  }, [])

  useSingleFlightPoll(load, { intervalMs })

  return { health, error, warning }
}
