import { act, renderHook } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { api } from '@/lib/api'
import { useProcesses } from './useProcesses'
import { useProjects } from './useProjects'

vi.mock('@/lib/api', () => ({
  api: {
    getProcesses: vi.fn(),
    getProjects: vi.fn(),
  },
}))

describe('initial data loading with automatic refresh disabled', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    vi.mocked(api.getProcesses).mockResolvedValue({ processes: [] })
    vi.mocked(api.getProjects).mockResolvedValue({ projects: [] })
  })

  afterEach(() => {
    vi.useRealTimers()
    vi.clearAllMocks()
  })

  it('loads processes once without starting an interval', async () => {
    renderHook(() => useProcesses(false, 100))
    await act(async () => Promise.resolve())
    expect(api.getProcesses).toHaveBeenCalledTimes(1)

    await act(async () => vi.advanceTimersByTimeAsync(1_000))
    expect(api.getProcesses).toHaveBeenCalledTimes(1)
  })

  it('loads projects once without starting an interval', async () => {
    renderHook(() => useProjects(false, 100))
    await act(async () => Promise.resolve())
    expect(api.getProjects).toHaveBeenCalledTimes(1)

    await act(async () => vi.advanceTimersByTimeAsync(1_000))
    expect(api.getProjects).toHaveBeenCalledTimes(1)
  })
})
