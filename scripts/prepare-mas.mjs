#!/usr/bin/env node
/**
 * Fill the Mac App Store entitlements from APPLE_TEAM_ID.
 * Usage: node scripts/prepare-mas.mjs <dev|build>
 * `build` also requires the provisioning profile on disk.
 */
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const mode = process.argv[2] === 'dev' ? 'dev' : 'build';
const team = process.env.APPLE_TEAM_ID ?? '';

if (!/^[A-Z0-9]{10}$/.test(team)) {
  console.error(`Mac App Store ${mode} needs APPLE_TEAM_ID (10 letters or digits).
Set it in the shell. It is not stored in the repo.
See docs/legal/STORE_ACCOUNT.md for the rest of the account data.`);
  process.exit(1);
}

const templatePath = path.join(root, 'src-tauri/entitlements/mas.entitlements');
const generatedPath = path.join(root, 'src-tauri/entitlements/mas.generated.entitlements');
const template = readFileSync(templatePath, 'utf8');
writeFileSync(generatedPath, template.replaceAll('TEAMID', team));

const profile = path.join(root, 'src-tauri/certs/DitherYuki_MAS.provisionprofile');
if (mode === 'build' && !existsSync(profile)) {
  console.error(`Missing ${profile}
Download the Mac App Store provisioning profile for com.dither.app and place it there. Do not commit it.`);
  process.exit(1);
}

if (mode === 'dev' && !existsSync(profile)) {
  console.warn(`No provisioning profile at ${profile}. A sandboxed dev run still needs one.`);
}
