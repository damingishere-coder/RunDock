// @group BusinessLogic : GitHub star prompt — shown once after first use, dismissible
import { useEffect, useState } from 'react'
import { Star, X } from 'lucide-react'

// @group Constants : Storage key + repo URL
const STORAGE_KEY = 'alter-pm2:github-star-dismissed'
const REPO_URL = 'https://github.com/damingishere-coder/RunDock'

// @group Utilities : Has the user already dismissed / acted on the banner?
function isDismissed(): boolean {
  try {
    return !!localStorage.getItem(STORAGE_KEY)
  } catch {
    return false
  }
}

function dismiss(reason: 'later' | 'now' | 'done') {
  try {
    localStorage.setItem(STORAGE_KEY, reason)
  } catch {
    // Storage is an optional persistence enhancement. The in-memory visible
    // state still closes the banner when privacy settings disable storage.
  }
}

// @group BusinessLogic > GitHubStarBanner : Floating bottom-right popup asking to star the repo
export function GitHubStarBanner() {
  const [visible, setVisible] = useState(false)

  // Show after a short delay on first visit (never show again once acted on)
  useEffect(() => {
    if (isDismissed()) return
    const t = setTimeout(() => setVisible(true), 8000)
    return () => clearTimeout(t)
  }, [])

  if (!visible) return null

  function handleNow() {
    dismiss('now')
    window.open(REPO_URL, '_blank', 'noopener,noreferrer')
    setVisible(false)
  }

  function handleLater() {
    dismiss('later')
    setVisible(false)
  }

  function handleDone() {
    dismiss('done')
    setVisible(false)
  }

  return (
    <div
      className="rundock-star-banner"
      style={{
        position: 'fixed',
        bottom: 36, // sit above the 22px status bar
        right: 16,
        zIndex: 9000,
        width: 280,
        background: 'var(--color-card)',
        border: '1px solid var(--color-border)',
        borderRadius: 10,
        boxShadow: '0 18px 48px rgba(31,72,126,0.18)',
        overflow: 'hidden',
        animation: 'rundock-slide-up 0.28s cubic-bezier(0.4,0,0.2,1)',
      }}
    >
      {/* Accent top bar */}
      <div style={{ height: 3, background: 'linear-gradient(90deg, #147bff, #43c8ff)' }} />

      {/* Body */}
      <div style={{ padding: '14px 16px 12px' }}>
        {/* Close (×) */}
        <button
          onClick={handleLater}
          title="稍后提醒"
          style={{
            position: 'absolute',
            top: 10,
            right: 10,
            width: 20,
            height: 20,
            padding: 0,
            background: 'transparent',
            border: 'none',
            cursor: 'pointer',
            color: 'var(--color-muted-foreground)',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            borderRadius: 4,
          }}
          onMouseEnter={e => {
            ;(e.currentTarget as HTMLElement).style.color = 'var(--color-foreground)'
          }}
          onMouseLeave={e => {
            ;(e.currentTarget as HTMLElement).style.color = 'var(--color-muted-foreground)'
          }}
        >
          <X size={13} />
        </button>

        {/* Icon + heading */}
        <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 8 }}>
          <div
            style={{
              width: 32,
              height: 32,
              borderRadius: 8,
              flexShrink: 0,
              background: 'color-mix(in srgb, var(--color-primary) 13%, transparent)',
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
            }}
          >
            <Star size={16} color="var(--color-primary)" fill="var(--color-primary)" />
          </div>
          <div>
            <div
              style={{
                fontSize: 13,
                fontWeight: 700,
                color: 'var(--color-foreground)',
                lineHeight: 1.3,
              }}
            >
              喜欢 RunDock 吗？
            </div>
            <div
              style={{
                fontSize: 11,
                color: 'var(--color-muted-foreground)',
                lineHeight: 1.4,
                marginTop: 1,
              }}
            >
              在 GitHub 点个星标会很有帮助 ⭐
            </div>
          </div>
        </div>

        {/* CTA buttons */}
        <div style={{ display: 'flex', gap: 6, marginTop: 12 }}>
          <button
            onClick={handleNow}
            style={{
              flex: 1,
              height: 30,
              fontSize: 12,
              fontWeight: 600,
              background: 'var(--color-primary)',
              color: '#fff',
              border: 'none',
              borderRadius: 6,
              cursor: 'pointer',
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
              gap: 5,
              transition: 'filter 0.15s',
            }}
            onMouseEnter={e => {
              ;(e.currentTarget as HTMLElement).style.filter = 'brightness(1.1)'
            }}
            onMouseLeave={e => {
              ;(e.currentTarget as HTMLElement).style.filter = 'none'
            }}
          >
            <Star size={12} fill="#fff" />
            现在去点星标
          </button>

          <button
            onClick={handleLater}
            style={{
              flex: 1,
              height: 30,
              fontSize: 12,
              fontWeight: 500,
              background: 'var(--color-secondary)',
              color: 'var(--color-foreground)',
              border: '1px solid var(--color-border)',
              borderRadius: 6,
              cursor: 'pointer',
              transition: 'background 0.15s',
            }}
            onMouseEnter={e => {
              ;(e.currentTarget as HTMLElement).style.background = 'var(--color-accent)'
            }}
            onMouseLeave={e => {
              ;(e.currentTarget as HTMLElement).style.background = 'var(--color-secondary)'
            }}
          >
            稍后
          </button>
        </div>

        {/* "Already done" micro-link */}
        <div style={{ textAlign: 'center', marginTop: 8 }}>
          <button
            onClick={handleDone}
            style={{
              background: 'none',
              border: 'none',
              cursor: 'pointer',
              padding: 0,
              fontSize: 10,
              color: 'var(--color-muted-foreground)',
              textDecoration: 'underline',
              textUnderlineOffset: 2,
              opacity: 0.6,
            }}
            onMouseEnter={e => {
              ;(e.currentTarget as HTMLElement).style.opacity = '1'
            }}
            onMouseLeave={e => {
              ;(e.currentTarget as HTMLElement).style.opacity = '0.6'
            }}
          >
            我已经点过星标了
          </button>
        </div>
      </div>

      {/* Slide-up keyframe injected once */}
      <style>{`
        @keyframes rundock-slide-up {
          from { opacity: 0; transform: translateY(16px); }
          to   { opacity: 1; transform: translateY(0); }
        }
      `}</style>
    </div>
  )
}

// @group BusinessLogic > GitHubStarWidget : Compact star-count chip for the status bar
export function GitHubStarWidget() {
  const [stars, setStars] = useState<number | null>(null)

  useEffect(() => {
    const controller = new AbortController()
    fetch('https://api.github.com/repos/damingishere-coder/RunDock', {
      signal: controller.signal,
    })
      .then(response => {
        if (!response.ok) throw new Error(`GitHub API returned HTTP ${response.status}`)
        return response.json()
      })
      .then((data: { stargazers_count?: number }) => {
        if (typeof data.stargazers_count === 'number') {
          setStars(data.stargazers_count)
        }
      })
      .catch(error => {
        if ((error as Error)?.name !== 'AbortError') {
          console.warn('GitHub star count is unavailable', error)
        }
      })
    return () => controller.abort()
  }, [])

  return (
    <a
      href={REPO_URL}
      target="_blank"
      rel="noopener noreferrer"
      title="在 GitHub 为 RunDock 点星标"
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: 4,
        padding: '0 8px',
        height: '100%',
        borderLeft: '1px solid var(--color-border)',
        textDecoration: 'none',
        color: 'var(--color-muted-foreground)',
        fontSize: 11,
        fontWeight: 500,
        opacity: 0.8,
        cursor: 'pointer',
        whiteSpace: 'nowrap',
      }}
      onMouseEnter={e => {
        ;(e.currentTarget as HTMLElement).style.opacity = '1'
        ;(e.currentTarget as HTMLElement).style.color = 'var(--color-primary)'
      }}
      onMouseLeave={e => {
        ;(e.currentTarget as HTMLElement).style.opacity = '0.8'
        ;(e.currentTarget as HTMLElement).style.color = 'var(--color-muted-foreground)'
      }}
    >
      <Star size={11} />
      {stars !== null && <span>{stars >= 1000 ? `${(stars / 1000).toFixed(1)}k` : stars}</span>}
    </a>
  )
}
