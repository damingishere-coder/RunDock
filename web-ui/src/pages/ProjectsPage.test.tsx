import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router-dom'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { api } from '@/lib/api'
import type { ProjectInfo } from '@/types'
import ProjectsPage from './ProjectsPage'

vi.mock('@/lib/api', () => ({
  api: {
    getPorts: vi.fn(),
    updateProject: vi.fn(),
    startProject: vi.fn(),
    stopProject: vi.fn(),
    restartProject: vi.fn(),
  },
}))

const project: ProjectInfo = {
  id: 'project-1',
  kind: 'managed',
  display_name: 'AI JobPilot',
  note: '投递牛马 - win本地',
  category: '常用',
  web_port: null,
  launch_uri: null,
  enabled: true,
  status: 'stopped',
  process_count: 2,
  active_process_count: 0,
  cpu_percent: 0,
  memory_bytes: 0,
  members: [],
}

function renderPage(reload = vi.fn()) {
  return {
    reload,
    ...render(
      <MemoryRouter initialEntries={['/processes']}>
        <ProjectsPage projects={[project]} error={null} reload={reload} />
      </MemoryRouter>
    ),
  }
}

describe('ProjectsPage project identity', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    vi.mocked(api.getPorts).mockResolvedValue({ ports: [] })
    vi.mocked(api.updateProject).mockResolvedValue(project)
  })

  it('keeps the project name as text and places the note beside it', async () => {
    const user = userEvent.setup()
    renderPage()

    const identity = screen.getByTestId('project-identity-project-1')
    const name = within(identity).getByText('AI JobPilot')
    expect(name.tagName).toBe('SPAN')
    expect(name.closest('button')).toBeNull()
    expect(within(identity).getByText('投递牛马 - win本地')).toBeVisible()

    await user.click(identity)
    expect(screen.queryByRole('textbox', { name: '编辑 AI JobPilot 备注' })).not.toBeInTheDocument()
    expect(screen.queryByRole('textbox', { name: /名称/ })).not.toBeInTheDocument()
  })

  it('updates only the note field', async () => {
    const user = userEvent.setup()
    const { reload } = renderPage()

    await user.click(screen.getByRole('button', { name: '编辑 AI JobPilot 备注' }))
    const input = screen.getByRole('textbox', { name: '编辑 AI JobPilot 备注' })
    await user.clear(input)
    await user.type(input, '  新备注  ')
    await user.click(screen.getByRole('button', { name: '保存 AI JobPilot 备注' }))

    await waitFor(() =>
      expect(api.updateProject).toHaveBeenCalledWith('project-1', { note: '新备注' })
    )
    expect(api.updateProject).toHaveBeenCalledTimes(1)
    expect(reload).toHaveBeenCalledTimes(1)
  })

  it('cancels note editing without sending an update', async () => {
    const user = userEvent.setup()
    renderPage()

    await user.click(screen.getByRole('button', { name: '编辑 AI JobPilot 备注' }))
    await user.type(screen.getByRole('textbox', { name: '编辑 AI JobPilot 备注' }), '不会保存')
    await user.click(screen.getByRole('button', { name: '取消编辑 AI JobPilot 备注' }))

    expect(api.updateProject).not.toHaveBeenCalled()
    expect(screen.queryByRole('textbox', { name: '编辑 AI JobPilot 备注' })).not.toBeInTheDocument()
  })

  it('opens only the configured web port and keeps all listeners in technical details', async () => {
    const user = userEvent.setup()
    const runningProject: ProjectInfo = {
      ...project,
      web_port: 6866,
      status: 'running',
      active_process_count: 2,
      members: [
        { id: 'backend', name: 'Backend', status: 'running', pid: 101, enabled: true },
        { id: 'frontend', name: 'Frontend', status: 'running', pid: 202, enabled: true },
      ],
    }
    vi.mocked(api.getPorts).mockResolvedValue({
      ports: [
        {
          pid: 501,
          port: 8888,
          protocol: 'TCP',
          local_address: '127.0.0.1:8888',
          remote_address: '',
          state: 'LISTENING',
          process_name: 'java.exe',
          ancestor_pids: [101],
        },
        {
          pid: 501,
          port: 35729,
          protocol: 'TCP',
          local_address: '127.0.0.1:35729',
          remote_address: '',
          state: 'LISTENING',
          process_name: 'java.exe',
          ancestor_pids: [101],
        },
        {
          pid: 502,
          port: 61135,
          protocol: 'TCP',
          local_address: '127.0.0.1:61135',
          remote_address: '',
          state: 'LISTENING',
          process_name: 'java.exe',
          ancestor_pids: [101],
        },
        {
          pid: 601,
          port: 6866,
          protocol: 'TCP',
          local_address: '127.0.0.1:6866',
          remote_address: '',
          state: 'LISTENING',
          process_name: 'node.exe',
          ancestor_pids: [202],
        },
        {
          pid: 601,
          port: 63119,
          protocol: 'TCP',
          local_address: '127.0.0.1:63119',
          remote_address: '',
          state: 'LISTENING',
          process_name: 'node.exe',
          ancestor_pids: [202],
        },
      ],
    })

    render(
      <MemoryRouter initialEntries={['/processes']}>
        <ProjectsPage projects={[runningProject]} error={null} reload={vi.fn()} />
      </MemoryRouter>
    )

    const link = await screen.findByRole('link', { name: '打开网页 http://127.0.0.1:6866/' })
    expect(link).toHaveTextContent('打开网页：6866')
    expect(screen.queryByRole('button', { name: '选择网页端口' })).not.toBeInTheDocument()

    await user.click(screen.getByRole('button', { name: '展开技术组件' }))
    const backend = screen.getByTestId('technical-member-backend')
    expect(within(backend).getByText(':8888')).toBeVisible()
    expect(within(backend).getByText(':35729')).toBeVisible()
    expect(within(backend).getByText(':61135')).toBeVisible()
    const frontend = screen.getByTestId('technical-member-frontend')
    expect(within(frontend).getByText(':6866')).toBeVisible()
    expect(within(frontend).getByText(':63119')).toBeVisible()
  })

  it('shows a desktop-only software entry without process controls or metrics', () => {
    const desktopProject: ProjectInfo = {
      ...project,
      id: 'wanmotai',
      kind: 'desktop',
      display_name: '万模台',
      note: 'AI 创作平台',
      launch_uri: 'wanmotai://open',
      status: 'desktop',
      process_count: 0,
      active_process_count: 0,
      members: [],
    }

    render(
      <MemoryRouter initialEntries={['/processes']}>
        <ProjectsPage projects={[desktopProject]} error={null} reload={vi.fn()} />
      </MemoryRouter>
    )

    expect(screen.getByRole('link', { name: '打开软件 wanmotai://open' })).toHaveAttribute(
      'href',
      'wanmotai://open'
    )
    expect(screen.getAllByText(/桌面软件/).length).toBeGreaterThan(0)
    expect(screen.queryByRole('link', { name: /打开网页/ })).not.toBeInTheDocument()
    expect(
      screen.queryByRole('button', { name: /启动|停止|重启|启用|停用/ })
    ).not.toBeInTheDocument()
    expect(screen.queryByTitle('展开技术组件')).not.toBeInTheDocument()
    expect(screen.queryByText('CPU')).not.toBeInTheDocument()
    expect(screen.queryByText('内存')).not.toBeInTheDocument()
  })
})
