import { describe, expect, it } from 'vitest'
import type { BulkNamespaceResult } from '@/lib/api'
import { namespaceStartError } from '@/lib/analytics'

function result(overrides: Partial<BulkNamespaceResult>): BulkNamespaceResult {
  return {
    status: 'complete',
    namespace: 'api',
    attempted: 1,
    succeeded: 1,
    failed: 0,
    processes: [],
    failures: [],
    persistence: { status: 'committed', error: null },
    ...overrides,
  }
}

describe('namespaceStartError', () => {
  it('reports persistence failure even when every runtime action succeeded', () => {
    expect(
      namespaceStartError('api', result({ persistence: { status: 'failed', error: 'disk full' } }))
    ).toContain('状态保存失败：disk full')
  })

  it('returns no error for a fully committed operation', () => {
    expect(namespaceStartError('api', result({}))).toBeNull()
  })
})
