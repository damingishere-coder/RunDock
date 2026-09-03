import type { ProcessInfo } from '@/types'

export function filterCronJobsByNamespace(
  processes: ProcessInfo[],
  namespaceFilter?: string | null
): ProcessInfo[] {
  return processes.filter(
    process =>
      process.cron !== null &&
      (!namespaceFilter || (process.namespace || 'default') === namespaceFilter)
  )
}
