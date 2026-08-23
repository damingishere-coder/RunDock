import { describe, expect, it } from 'vitest'
import { projectActionError, projectStatusLabel, sortProjects } from './projects'
import type { ProjectActionResponse, ProjectInfo } from '@/types'

function project(id: string, name: string, category: string): ProjectInfo {
  return {
    id,
    kind: 'managed',
    display_name: name,
    note: '',
    category,
    web_port: null,
    launch_uri: null,
    enabled: true,
    status: 'stopped',
    process_count: 1,
    active_process_count: 0,
    cpu_percent: 0,
    memory_bytes: 0,
    members: [],
  }
}

describe('project presentation', () => {
  it('sorts common before custom and pending last', () => {
    const result = sortProjects([
      project('3', '知乎', '待定'),
      project('2', '实验', '开发'),
      project('1', '常用项目', '常用'),
    ])
    expect(result.map(item => item.category)).toEqual(['常用', '开发', '待定'])
  })

  it('uses project-level Chinese status labels', () => {
    expect(projectStatusLabel('partial')).toBe('部分运行')
    expect(projectStatusLabel('disabled')).toBe('已停用')
    expect(projectStatusLabel('desktop')).toBe('桌面软件')
  })

  it('keeps component failures visible', () => {
    const response: ProjectActionResponse = {
      project_id: 'p1', action: 'stop', success: false,
      results: [{ process_id: 'c1', name: 'Backend', success: false, error: 'PID 仍然存在' }],
    }
    expect(projectActionError(response)).toContain('Backend：PID 仍然存在')
  })
})
