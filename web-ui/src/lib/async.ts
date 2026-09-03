// @group Utilities : Run asynchronous work with a fixed concurrency ceiling

export async function mapSettledWithConcurrency<T, R>(
  items: readonly T[],
  limit: number,
  mapper: (item: T, index: number) => Promise<R>
): Promise<PromiseSettledResult<R>[]> {
  if (items.length === 0) return []

  const normalizedLimit = Number.isFinite(limit) ? Math.floor(limit) : 1
  const workerCount = Math.min(items.length, Math.max(1, Math.min(normalizedLimit, 64)))
  const results = new Array<PromiseSettledResult<R>>(items.length)
  let nextIndex = 0

  async function worker() {
    while (nextIndex < items.length) {
      const index = nextIndex++
      try {
        results[index] = { status: 'fulfilled', value: await mapper(items[index], index) }
      } catch (reason) {
        results[index] = { status: 'rejected', reason }
      }
    }
  }

  await Promise.all(Array.from({ length: workerCount }, worker))
  return results
}

export function waitForAbortableDelay(delayMs: number, signal: AbortSignal): Promise<void> {
  if (signal.aborted) return Promise.reject(new DOMException('Aborted', 'AbortError'))
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      signal.removeEventListener('abort', onAbort)
      resolve()
    }, delayMs)
    const onAbort = () => {
      clearTimeout(timer)
      reject(new DOMException('Aborted', 'AbortError'))
    }
    signal.addEventListener('abort', onAbort, { once: true })
  })
}
