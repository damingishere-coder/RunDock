import { act, renderHook } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { useSingleFlightPoll } from './useSingleFlightPoll'

afterEach(() => {
  vi.useRealTimers()
})

describe('useSingleFlightPoll', () => {
  it('waits for a request before scheduling the next poll', async () => {
    vi.useFakeTimers()
    let release: (() => void) | undefined
    let concurrent = 0
    let maxConcurrent = 0
    const task = vi.fn(async () => {
      concurrent += 1
      maxConcurrent = Math.max(maxConcurrent, concurrent)
      await new Promise<void>(resolve => {
        release = resolve
      })
      concurrent -= 1
    })

    renderHook(() => useSingleFlightPoll(task, { intervalMs: 100 }))
    await act(async () => Promise.resolve())
    expect(task).toHaveBeenCalledTimes(1)

    await act(async () => vi.advanceTimersByTimeAsync(1_000))
    expect(task).toHaveBeenCalledTimes(1)
    expect(maxConcurrent).toBe(1)

    await act(async () => {
      release?.()
      await Promise.resolve()
    })
    await act(async () => vi.advanceTimersByTimeAsync(100))
    expect(task).toHaveBeenCalledTimes(2)
    expect(maxConcurrent).toBe(1)
  })

  it('invalidates results after unmount', async () => {
    let release: (() => void) | undefined
    let resultWasCurrent: boolean | undefined
    let requestSignal: AbortSignal | undefined
    const task = vi.fn(async (isCurrent: () => boolean, signal: AbortSignal) => {
      requestSignal = signal
      await new Promise<void>(resolve => {
        release = resolve
      })
      resultWasCurrent = isCurrent()
    })

    const { unmount } = renderHook(() => useSingleFlightPoll(task, { intervalMs: 100 }))
    await act(async () => Promise.resolve())
    unmount()
    release?.()
    await act(async () => Promise.resolve())

    expect(resultWasCurrent).toBe(false)
    expect(requestSignal?.aborted).toBe(true)
  })

  it('does not revive an old scheduler after being disabled mid-flight', async () => {
    vi.useFakeTimers()
    let release: (() => void) | undefined
    const task = vi.fn(
      () =>
        new Promise<void>(resolve => {
          release = resolve
        })
    )

    const { rerender } = renderHook(
      ({ enabled }) => useSingleFlightPoll(task, { intervalMs: 100, enabled }),
      { initialProps: { enabled: true } }
    )
    await act(async () => Promise.resolve())
    expect(task).toHaveBeenCalledTimes(1)

    rerender({ enabled: false })
    await act(async () => {
      release?.()
      await Promise.resolve()
      await vi.advanceTimersByTimeAsync(1_000)
    })

    expect(task).toHaveBeenCalledTimes(1)
  })

  it('backs off after failures', async () => {
    vi.useFakeTimers()
    const task = vi.fn().mockRejectedValue(new Error('offline'))

    renderHook(() => useSingleFlightPoll(task, { intervalMs: 100 }))
    await act(async () => Promise.resolve())
    expect(task).toHaveBeenCalledTimes(1)

    await act(async () => vi.advanceTimersByTimeAsync(199))
    expect(task).toHaveBeenCalledTimes(1)
    await act(async () => vi.advanceTimersByTimeAsync(1))
    expect(task).toHaveBeenCalledTimes(2)
  })

  it('clamps invalid intervals instead of busy-looping', async () => {
    vi.useFakeTimers()
    const task = vi.fn().mockResolvedValue(undefined)

    renderHook(() => useSingleFlightPoll(task, { intervalMs: 0 }))
    await act(async () => Promise.resolve())
    expect(task).toHaveBeenCalledTimes(1)

    await act(async () => vi.advanceTimersByTimeAsync(999))
    expect(task).toHaveBeenCalledTimes(1)
    await act(async () => vi.advanceTimersByTimeAsync(1))
    expect(task).toHaveBeenCalledTimes(2)
  })
})
