import { fireEvent, render, screen } from '@testing-library/react'
import { beforeEach, describe, expect, it } from 'vitest'
import { ServerSwitcher } from './ServerSwitcher'

describe('ServerSwitcher storage recovery', () => {
  beforeEach(() => {
    localStorage.clear()
  })

  it('keeps the shell usable and offers a local reset for corrupt storage', () => {
    localStorage.setItem('alter_servers', '{broken')

    render(<ServerSwitcher />)
    fireEvent.click(screen.getByTitle('切换服务器'))

    expect(screen.getByRole('alert')).toHaveTextContent('服务器配置已损坏')
    fireEvent.click(screen.getByRole('button', { name: '重置为本地服务器' }))

    expect(localStorage.getItem('alter_servers')).toBeNull()
    expect(screen.queryByRole('alert')).not.toBeInTheDocument()
  })
})
