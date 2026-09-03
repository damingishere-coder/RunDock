import { fireEvent, render, screen } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { describe, expect, it, vi } from 'vitest'
import { Clock } from 'lucide-react'
import { CronJobSubmenu, NavRowWithAdd, SidebarAction, SidebarProjectGroup } from './AppSidebar'
import type { ProcessInfo, ProjectInfo } from '@/types'

describe('AppSidebar accessibility and navigation', () => {
  it('exposes cron namespace filtering and safely encodes links', () => {
    const processes = Array.from({ length: 5 }, (_, index) => ({
      name: `cron-${index}`,
      namespace: index === 0 ? 'billing/ops' : `namespace-${index}`,
      cron: '* * * * *',
    })) as ProcessInfo[]

    render(
      <MemoryRouter>
        <CronJobSubmenu processes={processes} currentNamespace="billing/ops" open />
      </MemoryRouter>
    )

    expect(screen.getByLabelText('筛选定时任务命名空间')).toBeVisible()
    expect(screen.getByRole('link', { name: /billing\/ops/ })).toHaveAttribute(
      'href',
      '/cron-jobs?namespace=billing%2Fops'
    )
  })

  it('reports the expanded state of collapsible navigation controls', () => {
    const project = {
      id: 'project-1',
      display_name: 'Project One',
      status: 'stopped',
      process_count: 1,
      kind: 'service',
    } as unknown as ProjectInfo

    render(
      <MemoryRouter>
        <NavRowWithAdd
          to="/cron-jobs"
          icon={Clock}
          label="定时任务"
          active={false}
          onAdd={vi.fn()}
          addTitle="新增"
          onToggleNs={vi.fn()}
          nsOpen={false}
        />
        <SidebarProjectGroup
          category="服务"
          projects={[project]}
          collapsed
          onToggle={vi.fn()}
          onNavigate={vi.fn()}
          onStop={vi.fn()}
          onRestart={vi.fn()}
          onError={vi.fn()}
        />
      </MemoryRouter>
    )

    expect(screen.getByRole('button', { name: '展开定时任务命名空间' })).toHaveAttribute(
      'aria-expanded',
      'false'
    )
    expect(screen.getByRole('button', { name: /服务/ })).toHaveAttribute('aria-expanded', 'false')
  })

  it('exposes text-labelled non-route tools with active state and count', () => {
    const onClick = vi.fn()
    render(
      <MemoryRouter>
        <SidebarAction icon={Clock} label="终端" active badge={2} onClick={onClick} />
      </MemoryRouter>
    )

    const action = screen.getByRole('button', { name: /终端/ })
    expect(action).toHaveAttribute('aria-pressed', 'true')
    expect(screen.getByLabelText('2 个已打开项')).toBeVisible()
    fireEvent.click(action)
    expect(onClick).toHaveBeenCalledOnce()
  })
})
