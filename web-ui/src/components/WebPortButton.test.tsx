import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it } from 'vitest'
import { WebPortButton } from './WebPortButton'
import type { RemoteServer } from '@/lib/servers'

const localServer: RemoteServer = {
  id: 'local',
  name: '本地',
  host: '127.0.0.1',
  port: 2999,
  connectionType: 'direct',
}

describe('WebPortButton', () => {
  it('renders nothing without a detected port', () => {
    const { container } = render(<WebPortButton ports={[]} server={localServer} showLabel />)
    expect(container).toBeEmptyDOMElement()
  })

  it('opens a single port directly in a new tab', () => {
    render(<WebPortButton ports={[5173]} server={localServer} showLabel />)
    const link = screen.getByRole('link', { name: '打开网页 http://127.0.0.1:5173/' })
    expect(link).toHaveAttribute('href', 'http://127.0.0.1:5173/')
    expect(link).toHaveAttribute('target', '_blank')
    expect(link).toHaveTextContent('网页')
  })

  it('offers every detected port in a chooser', async () => {
    const user = userEvent.setup()
    render(<WebPortButton ports={[5173, 8766]} server={localServer} showLabel />)

    await user.click(screen.getByRole('button', { name: '选择网页端口' }))

    expect(screen.getByRole('menuitem', { name: ':5173打开' })).toHaveAttribute('href', 'http://127.0.0.1:5173/')
    expect(screen.getByRole('menuitem', { name: ':8766打开' })).toHaveAttribute('href', 'http://127.0.0.1:8766/')
  })

  it('opens only the configured project web port without a chooser', () => {
    render(<WebPortButton ports={[6866, 8888, 35729, 61135, 63119]} preferredPort={6866} server={localServer} showLabel />)

    const link = screen.getByRole('link', { name: '打开网页 http://127.0.0.1:6866/' })
    expect(link).toHaveAttribute('href', 'http://127.0.0.1:6866/')
    expect(link).toHaveTextContent('打开网页：6866')
    expect(screen.queryByRole('button', { name: '选择网页端口' })).not.toBeInTheDocument()
  })

  it('hides a configured project web port until that port is listening', () => {
    const { container } = render(<WebPortButton ports={[8888]} preferredPort={6866} server={localServer} showLabel />)
    expect(container).toBeEmptyDOMElement()
  })

  it('disables direct opening for an SSH connection', () => {
    render(<WebPortButton ports={[5173]} server={{ ...localServer, id: 'ssh', connectionType: 'ssh', sshHost: 'example.invalid' }} showLabel />)
    expect(screen.getByRole('button', { name: '网页（SSH 连接不可直接打开）' })).toBeDisabled()
  })
})
