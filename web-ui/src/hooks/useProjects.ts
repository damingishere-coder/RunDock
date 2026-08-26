// @group BusinessLogic : Poll logical projects at the same cadence as process data

import { useCallback, useState } from 'react'
import { api } from '@/lib/api'
import type { ProjectInfo } from '@/types'
import { useSingleFlightPoll } from './useSingleFlightPoll'
import { isProjectInfo } from '@/lib/schemas'

export function useProjects(autoRefresh = true, intervalMs = 3000) {
  const [projects, setProjects] = useState<ProjectInfo[]>([])
  const [error, setError] = useState<string | null>(null)

  const load = useCallback(async (isCurrent: () => boolean, signal: AbortSignal) => {
    try {
      const data = await api.getProjects({ signal })
      if (!isCurrent()) return
      if (!Array.isArray(data.projects) || !data.projects.every(isProjectInfo)) {
        throw new Error('项目列表响应格式无效')
      }
      setProjects(data.projects)
      setError(null)
    } catch (loadError) {
      if (isCurrent()) {
        setError(loadError instanceof Error ? loadError.message : 'disconnected')
      }
      throw loadError
    }
  }, [])

  const reload = useSingleFlightPoll(load, { intervalMs, enabled: autoRefresh })

  return { projects, error, reload }
}
