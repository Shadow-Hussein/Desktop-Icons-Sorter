import { useEffect, useState } from 'react'
import type { PanelState } from '../../shared/types'

/** Current panel config, kept in sync with the main process. */
export function usePanelState(): PanelState | null {
  const [state, setState] = useState<PanelState | null>(null)
  useEffect(() => {
    const off = window.sorter.onStateChanged(setState)
    void window.sorter.getState().then((s) => s && setState(s))
    return off
  }, [])
  return state
}
