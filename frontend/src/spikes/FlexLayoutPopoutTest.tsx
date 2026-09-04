import React, { useCallback, useState, useEffect } from 'react';

/**
 * B2 Spike: FlexLayout Popout Test
 * 
 * Purpose: Determine if window.open() works inside Tauri WebView
 * 
 * Outcomes:
 * - Path A: Native window.open() works (4–5 hrs Phase 3)
 * - Path B: Requires Tauri bridge (8–10 hrs Phase 3)
 * - Path C: Multiple workarounds needed (40+ hrs Phase 3, escalate)
 */

export const FlexLayoutPopoutTest: React.FC = () => {
  const [testLog, setTestLog] = useState<string[]>([]);
  const [windowOpened, setWindowOpened] = useState(false);

  // Helper to log test events
  const log = useCallback((message: string) => {
    const timestamp = new Date().toLocaleTimeString();
    console.log(`[${timestamp}] ${message}`);
    setTestLog((prev) => [...prev, `[${timestamp}] ${message}`]);
  }, []);

  useEffect(() => {
    log('Spike test component mounted. Ready for popout testing.');
  }, [log]);

  /**
   * Test A: Basic Popout
   * - Try to open a new window using window.open()
   * - Observe if window appears and where
   */
  const testBasicPopout = useCallback(() => {
    log('TEST A: Starting basic popout test...');

    try {
      const width = 600;
      const height = 400;
      const left = window.screenX + 100;
      const top = window.screenY + 100;

      const features = `width=${width},height=${height},left=${left},top=${top}`;
      const newWindow = window.open('/spike-popout?test=basic', 'spike-popout-test', features);

      if (newWindow) {
        setWindowOpened(true);
        log(`✅ TEST A.1: window.open() succeeded`);
        log(`  - Window object: ${typeof newWindow}`);
        log(`  - Window name: ${newWindow.name}`);
        log(`  - Position: left=${left}, top=${top}`);
        log(`  - Size: ${width}x${height}`);
      } else {
        log(`❌ TEST A.1: window.open() returned null (popup blocked?)`);
      }
    } catch (error) {
      log(`❌ TEST A.1: Exception caught: ${error instanceof Error ? error.message : String(error)}`);
    }
  }, [log]);

  /**
   * Test B: IPC in Popout
   * - Try to call Tauri IPC from main window
   * - Verify IPC works before popout (baseline)
   */
  const testIPCBaseline = useCallback(async () => {
    log('TEST B (Baseline): Testing IPC from main window...');

    try {
      // Simulate IPC call (would need actual Tauri setup)
      // For now, just check if Tauri API is available
      if ((window as any).__TAURI__) {
        log(`✅ TEST B.0: Tauri API available in main window`);
      } else {
        log(`⚠️ TEST B.0: Tauri API NOT available (normal in dev, expected in Tauri)`);
      }
    } catch (error) {
      log(`❌ TEST B.0: Error checking Tauri: ${error instanceof Error ? error.message : String(error)}`);
    }
  }, [log]);

  /**
   * Test C: Styling Check
   * - Check if CSS is loaded in main window
   * - Verify computed styles on test element
   */
  const testStylingBaseline = useCallback(() => {
    log('TEST C (Baseline): Checking styling in main window...');

    try {
      const elem = document.querySelector('[data-spike-test="style-check"]');
      if (elem) {
        const styles = window.getComputedStyle(elem);
        log(`✅ TEST C.0: Computed styles available`);
        log(`  - Color: ${styles.color}`);
        log(`  - Font-family: ${styles.fontFamily}`);
      } else {
        log(`⚠️ TEST C.0: No test element found (will add one)`);
      }
    } catch (error) {
      log(`❌ TEST C.0: Error checking styles: ${error instanceof Error ? error.message : String(error)}`);
    }
  }, [log]);

  /**
   * Test D: Window Bounds
   * - Check available window bounds APIs
   */
  const testBoundsAPIs = useCallback(() => {
    log('TEST D: Checking window bounds APIs...');

    try {
      log(`✅ TEST D.0: Window bounds available`);
      log(`  - window.innerWidth: ${window.innerWidth}`);
      log(`  - window.innerHeight: ${window.innerHeight}`);
      log(`  - window.screenX: ${window.screenX}`);
      log(`  - window.screenY: ${window.screenY}`);
      log(`  - window.screen.width: ${window.screen.width}`);
      log(`  - window.screen.height: ${window.screen.height}`);
    } catch (error) {
      log(`❌ TEST D.0: Error checking bounds: ${error instanceof Error ? error.message : String(error)}`);
    }
  }, [log]);

  /**
   * Test E: DevTools Status
   * - Check if DevTools is available
   */
  const testDevTools = useCallback(() => {
    log('TEST E: Checking DevTools availability...');

    try {
      // Check for common DevTools indicators
      const isDevToolsOpen = /chrome|firefox|safari/i.test(navigator.userAgent) && (window as any).devtools?.open;
      
      log(`⚠️ TEST E.0: DevTools detection (basic check only)`);
      log(`  - Navigator userAgent: ${navigator.userAgent.substring(0, 100)}`);
      log(`  - Check DevTools manually: Cmd+Option+I (macOS) or Ctrl+Shift+I (Windows)`);
    } catch (error) {
      log(`❌ TEST E.0: Error checking DevTools: ${error instanceof Error ? error.message : String(error)}`);
    }
  }, [log]);

  /**
   * Run all baseline tests
   */
  const runAllTests = useCallback(async () => {
    setTestLog([]); // Clear previous logs
    log('═══════════════════════════════════════');
    log('B2 SPIKE: FLEXTLAYOUT POPOUT TEST');
    log('═══════════════════════════════════════');
    log('');

    testBasicPopout();
    await new Promise((resolve) => setTimeout(resolve, 500));

    testIPCBaseline();
    await new Promise((resolve) => setTimeout(resolve, 500));

    testStylingBaseline();
    await new Promise((resolve) => setTimeout(resolve, 500));

    testBoundsAPIs();
    await new Promise((resolve) => setTimeout(resolve, 500));

    testDevTools();

    log('');
    log('═══════════════════════════════════════');
    log('INSTRUCTIONS:');
    log('1. Open DevTools: Cmd+Option+I (macOS) or Ctrl+Shift+I (Windows)');
    log('2. Check Console for logs above ✅ or ❌');
    log('3. If window opened: inspect popout window console too');
    log('4. Copy console output and save to spike-evidence/');
    log('═══════════════════════════════════════');
  }, [log, testBasicPopout, testIPCBaseline, testStylingBaseline, testBoundsAPIs, testDevTools]);

  const clearLog = useCallback(() => {
    setTestLog([]);
    log('Log cleared');
  }, [log]);

  return (
    <div
      style={{
        height: '100vh',
        display: 'flex',
        flexDirection: 'column',
        background: '#f5f5f5',
        fontFamily: 'monospace',
      }}
    >
      {/* Header */}
      <div style={{ padding: '20px', background: '#2c3e50', color: '#ecf0f1' }}>
        <h1 style={{ margin: '0 0 10px 0' }}>🔬 B2 Spike: FlexLayout Popout Test</h1>
        <p style={{ margin: '0', fontSize: '14px' }}>
          Testing: Does window.open() work inside Tauri WebView?
        </p>
        <p style={{ margin: '5px 0 0 0', fontSize: '12px', color: '#95a5a6' }}>
          Expected duration: 1–2 hours | Outcome: Path A/B/C
        </p>
      </div>

      {/* Controls */}
      <div style={{ padding: '15px', background: '#ecf0f1', borderBottom: '1px solid #bdc3c7', display: 'flex', gap: '10px' }}>
        <button
          onClick={runAllTests}
          style={{
            padding: '10px 20px',
            background: '#3498db',
            color: 'white',
            border: 'none',
            borderRadius: '4px',
            cursor: 'pointer',
            fontWeight: 'bold',
          }}
        >
          ▶ Run All Tests
        </button>
        <button
          onClick={testBasicPopout}
          style={{
            padding: '10px 20px',
            background: '#2ecc71',
            color: 'white',
            border: 'none',
            borderRadius: '4px',
            cursor: 'pointer',
          }}
        >
          Test A: Basic Popout
        </button>
        <button
          onClick={testIPCBaseline}
          style={{
            padding: '10px 20px',
            background: '#e74c3c',
            color: 'white',
            border: 'none',
            borderRadius: '4px',
            cursor: 'pointer',
          }}
        >
          Test B: IPC
        </button>
        <button
          onClick={clearLog}
          style={{
            padding: '10px 20px',
            background: '#95a5a6',
            color: 'white',
            border: 'none',
            borderRadius: '4px',
            cursor: 'pointer',
          }}
        >
          Clear Log
        </button>
        {windowOpened && (
          <div style={{ padding: '10px', background: '#2ecc71', color: 'white', borderRadius: '4px' }}>
            ✅ Popout Window Opened
          </div>
        )}
      </div>

      {/* Log Output */}
      <div
        style={{
          flex: 1,
          overflow: 'auto',
          padding: '15px',
          background: '#fff',
          fontFamily: 'Courier New, monospace',
          fontSize: '13px',
          lineHeight: '1.6',
          whiteSpace: 'pre-wrap',
          wordBreak: 'break-word',
        }}
      >
        {testLog.length === 0 ? (
          <div style={{ color: '#7f8c8d' }}>
            📋 No logs yet. Click "Run All Tests" to start spike execution.
            <br />
            <br />
            Each test will be logged here. Check browser DevTools Console as well.
          </div>
        ) : (
          testLog.map((line, idx) => (
            <div
              key={idx}
              style={{
                color: line.includes('✅') ? '#27ae60' : line.includes('❌') ? '#e74c3c' : line.includes('⚠️') ? '#f39c12' : '#2c3e50',
              }}
            >
              {line}
            </div>
          ))
        )}
      </div>

      {/* Instructions */}
      <div style={{ padding: '15px', background: '#ecf0f1', borderTop: '1px solid #bdc3c7', fontSize: '12px', color: '#7f8c8d' }}>
        <strong>📌 Quick Guide:</strong>
        <br />
        1. Click "Run All Tests" to execute all test cases (A–E)
        <br />
        2. Open DevTools to see console logs: Cmd+Option+I (macOS) | Ctrl+Shift+I (Windows)
        <br />
        3. If popout window appears: click on it and open its DevTools to check IPC/styling
        <br />
        4. After tests complete, save this log + DevTools console output to spike-evidence/
      </div>

      {/* Hidden element for style checking */}
      <div data-spike-test="style-check" style={{ display: 'none' }} />
    </div>
  );
};

export default FlexLayoutPopoutTest;
