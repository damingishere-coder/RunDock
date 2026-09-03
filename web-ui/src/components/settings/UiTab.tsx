// @group BusinessLogic > UiTab : UI settings — process row action visibility

import type { AppSettings } from '@/lib/settings'
import { SettingRow, Toggle } from './shared'
import { card, descStyle, sectionTitle } from './sharedStyles'

interface Props {
  settings: AppSettings
  onUpdate: (patch: Partial<AppSettings>) => void
}

export default function UiTab({ settings, onUpdate }: Props) {
  return (
    <>
      <p style={sectionTitle}>进程行操作</p>
      <div style={{ ...card, marginBottom: 8 }}>
        <p style={{ ...descStyle, marginBottom: 12 }}>
          启动 / 停止 / 重启始终可见。选择要内联显示的其他操作，其余操作会收纳到 <strong>⋯</strong>{' '}
          菜单中。
        </p>
        {[
          { key: 'logs', label: '日志', description: '打开进程日志查看器。' },
          { key: 'edit', label: '编辑', description: '编辑进程配置。' },
          { key: 'terminal', label: '终端', description: '在进程工作目录中打开终端。' },
          { key: 'env', label: '.env', description: '查看和编辑环境变量。' },
          {
            key: 'enable',
            label: '启用 / 停用',
            description: '切换是否将进程包含在“启动全部”中。',
          },
          { key: 'notify', label: '通知', description: '配置进程通知。' },
          { key: 'clone', label: '克隆', description: '复制此进程。' },
          { key: 'delete', label: '删除', description: '删除此进程。' },
        ].map(({ key, label, description }, i, arr) => (
          <SettingRow
            key={key}
            label={label}
            description={description}
            isLast={i === arr.length - 1}
            control={
              <Toggle
                checked={settings.visibleRowActions.includes(key)}
                onChange={v => {
                  const next = v
                    ? [...settings.visibleRowActions, key]
                    : settings.visibleRowActions.filter(k => k !== key)
                  onUpdate({ visibleRowActions: next })
                }}
              />
            }
          />
        ))}
      </div>
      <p
        style={{
          fontSize: 11,
          color: 'var(--color-muted-foreground)',
          textAlign: 'center',
          marginTop: 8,
        }}
      >
        更改会立即生效。
      </p>
    </>
  )
}
