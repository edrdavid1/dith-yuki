#!/usr/bin/env node
/**
 * Single entry for channel-aware tauri dev/build.
 * Usage: node scripts/tauri.mjs <dev|build> [direct|mas|msstore] [extra tauri args]
 * `dev` and `build` with no channel use direct. Other tauri subcommands pass through.
 */
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const argv = process.argv.slice(2);
const mode = argv[0];
const plat = process.platform;
const channels = ['direct', 'mas', 'msstore'];
const ok = {
  direct: ['darwin', 'win32'],
  mas: ['darwin'],
  msstore: ['win32'],
};

if (mode !== 'dev' && mode !== 'build') {
  const forwarded = spawnSync('npx', ['tauri', ...argv], {
    stdio: 'inherit',
    shell: plat === 'win32',
  });
  process.exit(forwarded.status ?? 1);
}

const channel = channels.includes(argv[1]) ? argv[1] : 'direct';
const extraStart = channels.includes(argv[1]) ? 2 : 1;
const extra = argv.slice(extraStart).filter((a) => a !== '--');

if (!['build', 'dev'].includes(mode) || !ok[channel]?.includes(plat)) {
  console.error(`Not allowed: ${mode} ${channel} on ${plat}`);
  process.exit(1);
}

if (channel === 'mas') {
  const prepare = path.join(path.dirname(fileURLToPath(import.meta.url)), 'prepare-mas.mjs');
  const prepared = spawnSync(process.execPath, [prepare, mode], {
    stdio: 'inherit',
    env: process.env,
  });
  if ((prepared.status ?? 1) !== 0) process.exit(prepared.status ?? 1);
}

const args = [
  'tauri',
  mode,
  '--features',
  `channel-${channel}`,
  '--config',
  `src-tauri/channels/${channel}.conf.json`,
  ...extra,
];

if (mode === 'build') {
  if (plat === 'darwin') args.push('--target', 'universal-apple-darwin');
  if (channel === 'mas') args.push('--bundles', 'app');
  if (channel === 'msstore') args.push('--no-bundle');
}

const r = spawnSync('npx', args, {
  stdio: 'inherit',
  shell: plat === 'win32',
  env: { ...process.env, VITE_CHANNEL: channel },
});
process.exit(r.status ?? 1);
