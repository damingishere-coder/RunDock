import type { BulkNamespaceResult } from './api'

export function namespaceStartError(ns: string, result: BulkNamespaceResult): string | null {
  const detail = result.failures[0]?.error ?? result.persistence.error
  if (result.persistence.status === 'failed') {
    return `命名空间 ${ns} 已启动，但状态保存失败${detail ? `：${detail}` : ''}`
  }
  if (result.status === 'partial') {
    return `命名空间 ${ns} 仅部分启动（成功 ${result.succeeded}，失败 ${result.failed}）${detail ? `：${detail}` : ''}`
  }
  return null
}
