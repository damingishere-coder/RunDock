import { Component, type ReactNode } from 'react'

interface Props {
  children: ReactNode
}

interface State {
  failed: boolean
}

export class AppErrorBoundary extends Component<Props, State> {
  state: State = { failed: false }

  static getDerivedStateFromError(): State {
    return { failed: true }
  }

  render() {
    if (!this.state.failed) return this.props.children

    return (
      <main
        role="alert"
        style={{
          minHeight: '100vh',
          display: 'grid',
          placeItems: 'center',
          padding: 24,
          background: 'var(--color-background)',
          color: 'var(--color-foreground)',
        }}
      >
        <div style={{ maxWidth: 460, textAlign: 'center' }}>
          <h1 style={{ fontSize: 20 }}>界面加载失败</h1>
          <p style={{ color: 'var(--color-muted-foreground)', lineHeight: 1.6 }}>
            页面资源可能已更新或临时不可用。请刷新后重试；守护进程不会因此停止。
          </p>
          <button
            type="button"
            onClick={() => window.location.reload()}
            style={{
              marginTop: 12,
              padding: '8px 16px',
              border: 0,
              borderRadius: 6,
              background: 'var(--color-primary)',
              color: '#fff',
              cursor: 'pointer',
            }}
          >
            刷新界面
          </button>
        </div>
      </main>
    )
  }
}
