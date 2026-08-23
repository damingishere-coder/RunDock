import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { isDesktopLaunchUri } from '@/lib/desktopLaunch'
import { DesktopLaunchButton } from './DesktopLaunchButton'

describe('DesktopLaunchButton', () => {
  it('renders a validated custom-protocol software entry', () => {
    render(<DesktopLaunchButton launchUri="wanmotai://open" />)
    expect(screen.getByRole('link', { name: '打开软件 wanmotai://open' })).toHaveAttribute(
      'href',
      'wanmotai://open'
    )
  })

  it('rejects web, file, script, credentialed, and malformed entries', () => {
    expect(isDesktopLaunchUri('https://example.com')).toBe(false)
    expect(isDesktopLaunchUri('file:///C:/Windows/System32/calc.exe')).toBe(false)
    expect(isDesktopLaunchUri('javascript://alert(1)')).toBe(false)
    expect(isDesktopLaunchUri('wanmotai://user@open')).toBe(false)
    expect(isDesktopLaunchUri('wanmotai:open')).toBe(false)
    const { container } = render(<DesktopLaunchButton launchUri="https://example.com" />)
    expect(container).toBeEmptyDOMElement()
  })
})
