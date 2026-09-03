import { render, screen } from '@testing-library/react'
import { MemoryRouter, Route, Routes } from 'react-router-dom'
import { describe, expect, it, vi } from 'vitest'
import { DEFAULT_SETTINGS } from '@/lib/settings'
import SettingsPage from './SettingsPage'

describe('SettingsPage server management', () => {
  it('routes the server tab to the inline local and remote manager', () => {
    localStorage.clear()
    render(
      <MemoryRouter initialEntries={['/settings/servers']}>
        <Routes>
          <Route
            path="/settings/:tab?"
            element={
              <SettingsPage settings={DEFAULT_SETTINGS} onUpdate={vi.fn()} onReset={vi.fn()} />
            }
          />
        </Routes>
      </MemoryRouter>
    )

    expect(screen.getByRole('tab', { name: '服务器' })).toHaveAttribute('aria-selected', 'true')
    expect(screen.getByRole('region', { name: '服务器连接设置' })).toBeVisible()
    expect(screen.getByText(/当前电脑默认连接本机 RunDock/)).toBeVisible()
  })
})
