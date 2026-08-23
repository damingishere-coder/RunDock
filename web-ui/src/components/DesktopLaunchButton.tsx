import { MonitorUp } from 'lucide-react'
import { isDesktopLaunchUri } from '@/lib/desktopLaunch'

export function DesktopLaunchButton({
  launchUri,
  ariaLabelPrefix,
}: {
  launchUri: string | null
  ariaLabelPrefix?: string
}) {
  if (!isDesktopLaunchUri(launchUri)) return null

  return (
    <a
      href={launchUri}
      aria-label={`${ariaLabelPrefix ? `${ariaLabelPrefix}：` : ''}打开软件 ${launchUri}`}
      title={`打开软件：${launchUri}`}
      style={{
        height: 28,
        display: 'inline-flex',
        alignItems: 'center',
        gap: 4,
        padding: '0 9px',
        borderRadius: 5,
        border: '1px solid color-mix(in srgb, var(--color-primary) 48%, var(--color-border))',
        background: 'color-mix(in srgb, var(--color-primary) 11%, var(--color-secondary))',
        color: 'var(--color-primary)',
        textDecoration: 'none',
        fontSize: 11,
        fontWeight: 600,
        whiteSpace: 'nowrap',
      }}
    >
      <MonitorUp size={12} />
      打开软件
    </a>
  )
}
