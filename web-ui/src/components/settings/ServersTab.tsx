// @group BusinessLogic > ServersTab : Local and remote RunDock server management

import { ServerSwitcher } from '@/components/ServerSwitcher'
import { sectionTitle } from './sharedStyles'

export default function ServersTab() {
  return (
    <>
      <p style={sectionTitle}>服务器连接</p>
      <p
        style={{
          margin: '-4px 0 14px',
          color: 'var(--color-muted-foreground)',
          fontSize: 12,
          lineHeight: 1.6,
        }}
      >
        当前电脑默认连接本机 RunDock。只有需要管理其他电脑时，才需要添加 HTTPS 直连或 SSH 隧道。
      </p>
      <ServerSwitcher variant="settings" />
    </>
  )
}
