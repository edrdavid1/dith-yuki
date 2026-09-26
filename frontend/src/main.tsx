import React from 'react'
import ReactDOM from 'react-dom/client'
import App from './App'
import { Providers } from './app/providers'
import { initPlatform } from './lib/platform'
import './shared/styles/tokens.css'
import './shared/styles/reset.css'
import './shared/styles/dockCorners.css'
import 'simplebar-react/dist/simplebar.min.css'
import './shared/styles/vendor/simplebar.css'
import './shared/styles/chrome/titlebar.css'

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
        <App />
      </Providers>
    </React.StrictMode>,
  )

  requestAnimationFrame(() => {
    requestAnimationFrame(dismissBootScreen)
  })
})()
