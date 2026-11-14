import React from 'react'
import ReactDOM from 'react-dom/client'
import App from './App'
import { Providers } from './app/providers'
import { bootChannel } from './channel'
import { initPlatform } from './lib/platform'
import { startBootGate } from './lib/boot'

function showChannelBootFailure(message: string): void {
  const pre = document.createElement('pre')
  pre.style.padding = '24px'
  pre.style.font = '14px/1.4 ui-monospace, monospace'
  pre.textContent = `Channel boot failed:\n${message}`
  document.body.replaceChildren(pre)
}
import './shared/styles/tokens.css'
import './shared/styles/reset.css'
import './shared/styles/dockCorners.css'
import 'simplebar-react/dist/simplebar.min.css'
import './shared/styles/vendor/simplebar.css'
import './shared/styles/chrome/titlebar.css'

;(async () => {
  // Splash stays up until App calls finishBoot() → Rust boot_finished.
  startBootGate()

  await initPlatform()

  // Fail loud on channel mismatch (rust feature vs VITE_CHANNEL) before UI mounts.
  try {
    await bootChannel()
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err)
    showChannelBootFailure(msg)
    throw err
  }

  ReactDOM.createRoot(document.getElementById('root')!).render(
    <React.StrictMode>
      <Providers>
        <App />
      </Providers>
    </React.StrictMode>,
  )
})()
