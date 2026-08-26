import { describe, expect, it } from 'vitest'
import { mapSettledWithConcurrency, waitForAbortableDelay } from './async'

describe('mapSettledWithConcurrency', () => {
  it('preserves input order and bounds active work', async () => {
    let active = 0
    let maxActive = 0
    const results = await mapSettledWithConcurrency([1, 2, 3, 4, 5], 2, async value => {
      active += 1
      maxActive = Math.max(maxActive, active)
      await new Promise(resolve => setTimeout(resolve, 5))
      active -= 1
      return value * 2
    })

    expect(maxActive).toBe(2)
    expect(results).toEqual([
      { status: 'fulfilled', value: 2 },
      { status: 'fulfilled', value: 4 },
      { status: 'fulfilled', value: 6 },
      { status: 'fulfilled', value: 8 },
      { status: 'fulfilled', value: 10 },
    ])
  })

  it('records rejection without cancelling remaining work', async () => {
    const results = await mapSettledWithConcurrency([1, 2, 3], 2, async value => {
      if (value === 2) throw new Error('boom')
      return value
    })

    expect(results[0]).toEqual({ status: 'fulfilled', value: 1 })
    expect(results[1].status).toBe('rejected')
    expect(results[2]).toEqual({ status: 'fulfilled', value: 3 })
  })

  it('normalizes non-finite concurrency limits', async () => {
    await expect(
      mapSettledWithConcurrency([1, 2], Number.NaN, async value => value)
    ).resolves.toEqual([
      { status: 'fulfilled', value: 1 },
      { status: 'fulfilled', value: 2 },
    ])
  })
})

describe('waitForAbortableDelay', () => {
  it('rejects immediately when a pending retry is cancelled', async () => {
    const controller = new AbortController()
    const pending = waitForAbortableDelay(60_000, controller.signal)

    controller.abort()

    await expect(pending).rejects.toMatchObject({ name: 'AbortError' })
  })
})
