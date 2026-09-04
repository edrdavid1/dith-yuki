import React, { useEffect, useState } from 'react';

/**
 * PopoutTestWindow - Target page for popout test
 * 
 * This component is loaded in a popout window.
 * Used to test if window.open() works in Tauri and if content renders correctly.
 */

export const PopoutTestWindow: React.FC = () => {
  const [windowInfo, setWindowInfo] = useState<any>({});
  const [ipcStatus, setIpcStatus] = useState<string>('Checking...');

  useEffect(() => {
    // Log that we're in a popout
    console.log('🪟 PopoutTestWindow loaded in popout window');

    // Collect window information
    const info = {
      name: window.name,
      screenX: window.screenX,
      screenY: window.screenY,
      innerWidth: window.innerWidth,
      innerHeight: window.innerHeight,
      screenAvailWidth: window.screen.availWidth,
      screenAvailHeight: window.screen.availHeight,
      userAgent: navigator.userAgent.substring(0, 50),
      isSecureContext: window.isSecureContext,
      hasTauri: !!(window as any).__TAURI__,
    };

    setWindowInfo(info);
    console.log('Window Info:', info);

    // Check if IPC works
    const checkIPC = async () => {
      try {
        if ((window as any).__TAURI__) {
          console.log('✅ Tauri API available in popout');
          setIpcStatus('✅ Tauri API available');

          // Try a simple IPC call (would need actual setup)
          // const result = await invoke('is_release_build');
          // setIpcStatus(`✅ IPC works: ${result}`);
        } else {
          console.log('❌ Tauri API NOT available in popout');
          setIpcStatus('❌ Tauri API NOT available');
        }
      } catch (error) {
        console.error('❌ IPC check failed:', error);
        setIpcStatus(`❌ Error: ${error instanceof Error ? error.message : String(error)}`);
      }
    };

    checkIPC();
  }, []);

  return (
    <div style={{ padding: '20px', fontFamily: 'monospace', background: '#f0f0f0', minHeight: '100vh' }}>
      <h2>🪟 Popout Test Window</h2>
      <p style={{ color: '#666' }}>
        This window was opened via window.open(). Check if styling, IPC, and window properties work correctly.
      </p>

      <div style={{ background: '#fff', padding: '15px', borderRadius: '4px', marginBottom: '15px' }}>
        <h3>Window Information</h3>
        <ul style={{ fontSize: '13px' }}>
          <li><strong>Window Name:</strong> {windowInfo.name}</li>
          <li><strong>Position:</strong> ({windowInfo.screenX}, {windowInfo.screenY})</li>
          <li><strong>Size:</strong> {windowInfo.innerWidth} × {windowInfo.innerHeight}</li>
          <li><strong>Available Screen:</strong> {windowInfo.screenAvailWidth} × {windowInfo.screenAvailHeight}</li>
          <li><strong>Secure Context:</strong> {String(windowInfo.isSecureContext)}</li>
          <li><strong>Tauri API Available:</strong> {String(windowInfo.hasTauri)}</li>
        </ul>
      </div>

      <div style={{ background: '#fff', padding: '15px', borderRadius: '4px', marginBottom: '15px' }}>
        <h3>IPC Status</h3>
        <p style={{ fontSize: '14px', color: ipcStatus.includes('✅') ? '#27ae60' : '#e74c3c' }}>
          {ipcStatus}
        </p>
      </div>

      <div style={{ background: '#fff', padding: '15px', borderRadius: '4px', marginBottom: '15px' }}>
        <h3>Styling Test</h3>
        <p>If this text appears with styling (blue color, monospace font), CSS is loaded correctly:</p>
        <p style={{ color: '#3498db', fontSize: '16px', fontWeight: 'bold' }}>
          ✅ Styling works!
        </p>
      </div>

      <div style={{ background: '#fff', padding: '15px', borderRadius: '4px', marginBottom: '15px' }}>
        <h3>Console Logs</h3>
        <p style={{ fontSize: '12px', color: '#7f8c8d' }}>
          Open DevTools Console (Cmd+Option+I or Ctrl+Shift+I) to see detailed logs of this popout window.
        </p>
        <button
          onClick={() => {
            console.log('Manual test log from popout window');
            alert('Check DevTools console for log');
          }}
          style={{
            padding: '10px 20px',
            background: '#3498db',
            color: 'white',
            border: 'none',
            borderRadius: '4px',
            cursor: 'pointer',
          }}
        >
          Send Test Log to Console
        </button>
      </div>

      <div style={{ background: '#fff', padding: '15px', borderRadius: '4px' }}>
        <h3>Closure Test</h3>
        <button
          onClick={() => {
            console.log('Popout closing...');
            window.close();
          }}
          style={{
            padding: '10px 20px',
            background: '#e74c3c',
            color: 'white',
            border: 'none',
            borderRadius: '4px',
            cursor: 'pointer',
          }}
        >
          Close This Window
        </button>
      </div>

      <p style={{ marginTop: '20px', fontSize: '12px', color: '#7f8c8d', textAlign: 'center' }}>
        B2 Spike Test | Check main window console for spike outcome (Path A/B/C)
      </p>
    </div>
  );
};

export default PopoutTestWindow;
