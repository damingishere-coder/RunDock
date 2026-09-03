import { act, renderHook } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import type { ProcessInfo, ProcessStatus } from '@/types'
import { useNotificationTray } from './useNotificationTray'

function processWith(status: ProcessStatus): ProcessInfo {
  return {
    id: 'api',
    project_id: null,
    name: 'api',
    namespace: 'default',
    status,
    pid: null,
    restart_count: 0,
    last_exit_code: 1,
  } as ProcessInfo
}

describe('useNotificationTray', () => {
  it('reports the crashed process status as a crash event', async () => {
    const { result, rerender } = renderHook(
      ({ status }: { status: ProcessStatus }) => useNotificationTray([processWith(status)]),
      { initialProps: { status: 'running' as ProcessStatus } }
    )

    await act(async () => rerender({ status: 'crashed' }))

    expect(result.current.notifications).toEqual([
      expect.objectContaining({ processId: 'api', event: 'crash', detail: '退出码：1' }),
    ])
  })
})
