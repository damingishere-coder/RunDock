// @group BusinessLogic : Reactive settings hook — reads/writes AppSettings via daemon REST API

import { useCallback, useEffect, useRef, useState } from 'react'
import {
  type AppSettings,
  DEFAULT_SETTINGS,
  loadSettings,
  saveSettings,
  resetSettings,
} from '@/lib/settings'

// @group BusinessLogic > useSettings : Returns settings state + mutators
export function useSettings() {
  const [settings, setSettings] = useState<AppSettings>({ ...DEFAULT_SETTINGS })
  const [loaded, setLoaded] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const settingsRef = useRef(settings)
  const saveQueueRef = useRef<Promise<void>>(Promise.resolve())
  const saveVersionRef = useRef(0)
  const writableRef = useRef(false)

  // @group BusinessLogic > Load : Fetch settings from daemon on mount
  useEffect(() => {
    loadSettings()
      .then(s => {
        settingsRef.current = s
        writableRef.current = true
        setSettings(s)
        setError(null)
      })
      .catch(loadError => {
        setError(loadError instanceof Error ? loadError.message : '加载设置失败')
      })
      .finally(() => setLoaded(true))
  }, [])

  // @group BusinessLogic > Update : Merge partial update and persist immediately
  const updateSettings = useCallback((patch: Partial<AppSettings>) => {
    if (!writableRef.current) {
      setError('设置尚未成功加载，暂不能保存')
      return
    }
    const next = { ...settingsRef.current, ...patch }
    const version = saveVersionRef.current + 1
    saveVersionRef.current = version
    settingsRef.current = next
    setSettings(next)
    saveQueueRef.current = saveQueueRef.current
      .catch(() => undefined)
      .then(() => saveSettings(next))
      .then(() => {
        if (saveVersionRef.current === version) setError(null)
      })
      .catch(saveError => {
        if (saveVersionRef.current === version) {
          setError(saveError instanceof Error ? saveError.message : '保存设置失败')
        }
      })
  }, [])

  // @group BusinessLogic > Reset : Restore all defaults
  const resetToDefaults = useCallback(async () => {
    if (!writableRef.current) {
      setError('设置尚未成功加载，暂不能重置')
      return
    }
    const version = saveVersionRef.current + 1
    saveVersionRef.current = version
    const resetRequest = saveQueueRef.current.catch(() => undefined).then(resetSettings)
    saveQueueRef.current = resetRequest.then(
      () => undefined,
      () => undefined
    )
    try {
      const defaults = await resetRequest
      if (saveVersionRef.current !== version) return
      settingsRef.current = defaults
      setSettings(defaults)
      setError(null)
    } catch (resetError) {
      if (saveVersionRef.current === version) {
        setError(resetError instanceof Error ? resetError.message : '重置设置失败')
      }
    }
  }, [])

  return { settings, updateSettings, resetToDefaults, loaded, error }
}
