import React from 'react'
import ReactDOM from 'react-dom/client'
import App from './App'
import { Providers } from './app/providers'
import { initPlatform } from './lib/platform'
import { startBootGate } from './lib/boot'
import './shared/styles/tokens.css'
import './shared/styles/reset.css'
import './shared/styles/dockCorners.css'
import 'simplebar-react/dist/simplebar.min.css'
import './shared/styles/vendor/simplebar.css'
import './shared/styles/chrome/titlebar.css'

;(async () => {
  // Hidden window + delayed splash gate. App calls finishBoot() when ready.
  startBootGate()

  await initPlatform()

  ReactDOM.createRoot(document.getElementById('root')!).render(
    <React.StrictMode>
      <Providers>
        <App />
      </Providers>
    </React.StrictMode>,
  )
})()
