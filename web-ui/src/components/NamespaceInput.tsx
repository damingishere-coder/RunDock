// @group BusinessLogic : Namespace text input with autocomplete from existing process namespaces

import { useEffect, useId, useState } from 'react'
import { api } from '@/lib/api'

interface NamespaceInputProps {
  value: string
  onChange: (value: string) => void
  style?: React.CSSProperties
  placeholder?: string
  spellCheck?: boolean
}

// @group BusinessLogic > NamespaceInput : Input + datalist — fetches namespace list once on mount
export function NamespaceInput({
  value,
  onChange,
  style,
  placeholder = 'default',
  spellCheck = false,
}: NamespaceInputProps) {
  const [namespaces, setNamespaces] = useState<string[]>([])
  const [suggestionError, setSuggestionError] = useState(false)
  const listId = useId()
  const errorId = `${listId}-error`

  useEffect(() => {
    const controller = new AbortController()
    api
      .getProcesses({ signal: controller.signal })
      .then(({ processes }) => {
        if (controller.signal.aborted) return
        const unique = [...new Set(processes.map(p => p.namespace || 'default'))].sort(
          (left, right) => left.localeCompare(right)
        )
        setNamespaces(unique)
        setSuggestionError(false)
      })
      .catch(() => {
        if (!controller.signal.aborted) setSuggestionError(true)
      })
    return () => controller.abort()
  }, [])

  return (
    <>
      <input
        list={listId}
        value={value}
        onChange={e => onChange(e.target.value)}
        style={style}
        placeholder={placeholder}
        spellCheck={spellCheck}
        autoComplete="off"
        aria-describedby={suggestionError ? errorId : undefined}
      />
      <datalist id={listId}>
        {namespaces.map(ns => (
          <option key={ns} value={ns} />
        ))}
      </datalist>
      {suggestionError && (
        <span id={errorId} role="status" style={{ fontSize: 11, color: 'var(--color-warning)' }}>
          命名空间建议暂不可用，可继续手动输入
        </span>
      )}
    </>
  )
}
