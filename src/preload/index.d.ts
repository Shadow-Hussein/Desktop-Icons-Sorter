import type { SorterApi } from '../shared/types'

declare global {
  interface Window {
    sorter: SorterApi
  }
}

export {}
