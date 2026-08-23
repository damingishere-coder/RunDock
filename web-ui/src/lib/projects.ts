// @group BusinessLogic : Project presentation helpers shared by the main list and sidebar

import type { ProjectActionResponse, ProjectInfo, ProjectStatus } from '@/types'

export const PROJECT_CATEGORIES = ['常用', '待定'] as const

export function projectCategoryRank(category: string): number {
  if (category === '常用') return 0
  if (category === '待定') return 2
  return 1
}

export function sortProjects(projects: ProjectInfo[]): ProjectInfo[] {
  return [...projects].sort((a, b) =>
    projectCategoryRank(a.category) - projectCategoryRank(b.category)
      || a.category.localeCompare(b.category, 'zh-CN')
      || a.display_name.localeCompare(b.display_name, 'zh-CN'),
  )
}

export function projectStatusLabel(status: ProjectStatus): string {
  switch (status) {
    case 'desktop': return '桌面软件'
    case 'running': return '运行中'
    case 'partial': return '部分运行'
    case 'stopped': return '已停止'
    case 'errored': return '异常'
    case 'disabled': return '已停用'
  }
}

export function projectStatusColor(status: ProjectStatus): string {
  switch (status) {
    case 'desktop': return 'var(--color-primary)'
    case 'running': return 'var(--color-status-running)'
    case 'partial': return '#f59e0b'
    case 'stopped': return 'var(--color-muted-foreground)'
    case 'errored': return 'var(--color-status-crashed)'
    case 'disabled': return '#64748b'
  }
}

export function projectActionError(response: ProjectActionResponse): string | null {
  if (response.success) return null
  const failures = response.results.filter(result => !result.success)
  return failures.map(result => `${result.name}：${result.error ?? '操作失败'}`).join('；')
}
