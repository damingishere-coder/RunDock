import { describe, expect, it } from 'vitest'
import type { ProcessInfo } from '@/types'
import { filterCronJobsByNamespace } from '@/lib/cronJobs'

function process(name: string, namespace: string, cron: string | null): ProcessInfo {
  return { name, namespace, cron } as ProcessInfo
}

describe('filterCronJobsByNamespace', () => {
  const processes = [
    process('default-cron', 'default', '0 * * * *'),
    process('billing-cron', 'billing/ops', '*/5 * * * *'),
    process('regular', 'billing/ops', null),
  ]

  it('preserves an encoded namespace selection and excludes non-cron processes', () => {
    expect(filterCronJobsByNamespace(processes, 'billing/ops').map(item => item.name)).toEqual([
      'billing-cron',
    ])
  })

  it('returns every cron process when no namespace is selected', () => {
    expect(filterCronJobsByNamespace(processes).map(item => item.name)).toEqual([
      'default-cron',
      'billing-cron',
    ])
  })
})
