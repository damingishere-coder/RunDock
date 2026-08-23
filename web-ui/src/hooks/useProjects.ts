// @group BusinessLogic : Poll logical projects at the same cadence as process data

import { useCallback, useEffect, useRef, useState } from 'react'
import { api } from '@/lib/api'
import type { ProjectInfo } from '@/types'

export function useProjects(autoRefresh = true, intervalMs = 3000) {
  const [projects, setProjects] = useState<ProjectInfo[]>([])
  const [error, setError] = useState<string | null>(null)
  const timerRef = useRef<ReturnType<typeof setInterval> | null>(null)

  const load = useCallback(async () => {
    try {
      const data = await api.getProjects()
      setProjects(data.projects ?? [])
      setError(null)
    } catch (loadError) {
      setError(loadError instanceof Error ? loadError.message : 'disconnected')
    }
  }, [])

  useEffect(() => {
    const initialLoad = window.setTimeout(() => void load(), 0)
    if (timerRef.current) clearInterval(timerRef.current)
    if (autoRefresh) timerRef.current = setInterval(load, intervalMs)
    return () => {
      window.clearTimeout(initialLoad)
      if (timerRef.current) clearInterval(timerRef.current)
    }
  }, [load, autoRefresh, intervalMs])

  return { projects, error, reload: load }
}
