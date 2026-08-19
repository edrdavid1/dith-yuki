import React from 'react'
import ReactDOM from 'react-dom/client'
import App from './App'
import PanelWindow from './components/PanelWindow'
import { Providers } from './app/providers'
import { initPlatform } from './lib/platform'
import './shared/styles/tokens.css'
import './shared/styles/reset.css'
import 'simplebar-react/dist/simplebar.min.css'
import './shared/styles/vendor/simplebar.css'
import './shared/styles/chrome/titlebar.css'

// Determine if this is a floating panel window
const params = new URLSearchParams(window.location.search)
const panelId = params.get('panel')
const KNOWN_PANELS = ['effect', 'layers', 'colorlab', 'preview', 'preferences']

const isPanel = panelId !== null && KNOWN_PANELS.includes(panelId)

function dismissBootScreen() {
  const boot = document.getElementById('boot-screen')
  if (!boot) return
  boot.classList.add('boot-screen-done')
  window.setTimeout(() => boot.remove(), 240)
}

;(async () => {
  await initPlatform()

  ReactDOM.createRoot(document.getElementById('root')!).render(
    <React.StrictMode>
      <Providers>
        {isPanel ? <PanelWindow panelId={panelId!} /> : <App />}
      </Providers>
    </React.StrictMode>,
  )

  if (!isPanel) {
    requestAnimationFrame(() => {
      requestAnimationFrame(dismissBootScreen)
    })
  } else {
    document.getElementById('boot-screen')?.remove()
  }
})()
