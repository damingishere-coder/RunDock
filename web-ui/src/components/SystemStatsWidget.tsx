// @group BusinessLogic : Draggable system resource widget

import { BarChart2 } from 'lucide-react'
import { useCallback, useEffect, useRef, useState } from 'react'
import { useSingleFlightPoll } from '@/hooks/useSingleFlightPoll'
import { api } from '@/lib/api'
import { isSystemStats } from '@/lib/schemas'
import type { SystemStats } from '@/types'

// @group BusinessLogic > SystemStatsWidget : Bar color helper and StatRow — declared outside component to avoid re-creation on render
function statsBarColor(pct: number) {
  if (pct >= 90) return 'var(--color-status-crashed)'
  if (pct >= 70) return '#f97316'
  return 'var(--color-primary)'
}

function StatRow({ label, pct, detail }: { label: string; pct: number; detail: string }) {
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
      <span
        style={{
          fontSize: 10,
          fontWeight: 700,
          color: 'var(--color-muted-foreground)',
          width: 32,
          flexShrink: 0,
        }}
      >
        {label}
      </span>
      <div
        style={{
          flex: 1,
          height: 5,
          background: 'var(--color-border)',
          borderRadius: 3,
          overflow: 'hidden',
        }}
      >
        <div
          style={{
            height: '100%',
            borderRadius: 3,
            width: `${Math.min(pct, 100).toFixed(1)}%`,
            background: statsBarColor(pct),
            transition: 'width 0.4s ease',
          }}
        />
      </div>
      <span
        style={{
          fontSize: 10,
          color: 'var(--color-muted-foreground)',
          flexShrink: 0,
          minWidth: 64,
          textAlign: 'right',
        }}
      >
        {detail}
      </span>
    </div>
  )
}

// @group BusinessLogic > SystemStatsWidget : Floating draggable CPU / RAM / GPU usage widget
export function SystemStatsWidget({ onClose }: { onClose: () => void }) {
  const [stats, setStats] = useState<SystemStats | null>(null)
  const [statsError, setStatsError] = useState<string | null>(null)

  // @group BusinessLogic > SystemStatsWidget : Dragging state
  const [pos, setPos] = useState({ x: window.innerWidth - 260, y: 80 })
  const dragging = useRef(false)
  const dragOffset = useRef({ x: 0, y: 0 })

  function onMouseDown(e: React.MouseEvent) {
    dragging.current = true
    dragOffset.current = { x: e.clientX - pos.x, y: e.clientY - pos.y }
    e.preventDefault()
  }

  useEffect(() => {
    function onMove(e: MouseEvent) {
      if (!dragging.current) return
      setPos({
        x: Math.max(0, Math.min(window.innerWidth - 240, e.clientX - dragOffset.current.x)),
        y: Math.max(0, Math.min(window.innerHeight - 120, e.clientY - dragOffset.current.y)),
      })
    }
    function onUp() {
      dragging.current = false
    }
    window.addEventListener('mousemove', onMove)
    window.addEventListener('mouseup', onUp)
    return () => {
      window.removeEventListener('mousemove', onMove)
      window.removeEventListener('mouseup', onUp)
    }
  }, [])

  const loadStats = useCallback(async (isCurrent: () => boolean, signal: AbortSignal) => {
    try {
      const nextStats = await api.getSystemStats({ signal })
      if (!isSystemStats(nextStats)) {
        throw new Error('守护进程返回的系统指标格式无效')
      }
      if (isCurrent()) {
        setStats(nextStats)
        setStatsError(null)
      }
    } catch (loadError) {
      if (isCurrent()) {
        setStatsError(loadError instanceof Error ? loadError.message : '系统指标加载失败')
      }
      throw loadError
    }
  }, [])
  useSingleFlightPoll(loadStats, { intervalMs: 3_000 })

  const ramPct =
    stats && stats.ram_total_bytes > 0 ? (stats.ram_used_bytes / stats.ram_total_bytes) * 100 : 0

  return (
    <div
      style={{
        position: 'fixed',
        left: pos.x,
        top: pos.y,
        zIndex: 500,
        width: 240,
        background: 'var(--color-card)',
        border: '1px solid var(--color-border)',
        borderRadius: 10,
        boxShadow: '0 4px 24px rgba(0,0,0,0.2)',
        overflow: 'hidden',
        userSelect: 'none',
      }}
    >
      {/* Drag handle / title bar */}
      <div
        onMouseDown={onMouseDown}
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 6,
          padding: '8px 12px',
          background: 'var(--color-secondary)',
          borderBottom: '1px solid var(--color-border)',
          cursor: 'grab',
        }}
      >
        <BarChart2 size={12} style={{ opacity: 0.7 }} />
        <span style={{ flex: 1, fontSize: 11, fontWeight: 600, color: 'var(--color-foreground)' }}>
          系统统计
        </span>
        <button
          type="button"
          onClick={onClose}
          aria-label="关闭系统统计"
          style={{
            width: 18,
            height: 18,
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            background: 'transparent',
            border: 'none',
            cursor: 'pointer',
            color: 'var(--color-muted-foreground)',
            fontSize: 14,
            borderRadius: 3,
            padding: 0,
          }}
          onMouseEnter={e => {
            e.currentTarget.style.color = 'var(--color-destructive)'
          }}
          onMouseLeave={e => {
            e.currentTarget.style.color = 'var(--color-muted-foreground)'
          }}
        >
          ×
        </button>
      </div>
      {statsError && (
        <div
          role="alert"
          style={{ padding: '6px 10px', fontSize: 10, color: 'var(--color-status-crashed)' }}
        >
          {statsError}；显示上次成功数据
        </div>
      )}

      {/* Stats */}
      <div style={{ padding: '10px 14px', display: 'flex', flexDirection: 'column', gap: 8 }}>
        {!stats ? (
          <div
            style={{
              fontSize: 11,
              color: 'var(--color-muted-foreground)',
              textAlign: 'center',
              padding: '4px 0',
            }}
          >
            连接中…
          </div>
        ) : (
          <>
            <StatRow
              label="CPU"
              pct={stats.cpu_percent}
              detail={`${stats.cpu_percent.toFixed(1)}%`}
            />
            <StatRow
              label="RAM"
              pct={ramPct}
              detail={`${(stats.ram_used_bytes / 1073741824).toFixed(1)} / ${(stats.ram_total_bytes / 1073741824).toFixed(1)} GB`}
            />
            {stats.gpu && (
              <StatRow
                label="GPU"
                pct={stats.gpu.utilization_percent}
                detail={`${stats.gpu.utilization_percent.toFixed(0)}%`}
              />
            )}
            {stats.gpu && (
              <StatRow
                label="VRAM"
                pct={(stats.gpu.vram_used_bytes / stats.gpu.vram_total_bytes) * 100}
                detail={`${(stats.gpu.vram_used_bytes / 1073741824).toFixed(1)} / ${(stats.gpu.vram_total_bytes / 1073741824).toFixed(1)} GB`}
              />
            )}
            {stats.gpu && (
              <div
                style={{
                  fontSize: 9,
                  color: 'var(--color-muted-foreground)',
                  marginTop: -2,
                  opacity: 0.7,
                }}
              >
                {stats.gpu.name}
              </div>
            )}
          </>
        )}
      </div>
    </div>
  )
}
