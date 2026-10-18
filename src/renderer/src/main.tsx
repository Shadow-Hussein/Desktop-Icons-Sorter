import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { AppSettingsView } from './components/AppSettingsView'
import { Panel } from './components/Panel'
import { SettingsView } from './components/SettingsView'
import './styles.css'

const { view } = window.sorter
document.body.classList.add(`view-${view}`)

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    {view === 'settings' ? <SettingsView /> : view === 'app' ? <AppSettingsView /> : <Panel />}
  </StrictMode>
)
