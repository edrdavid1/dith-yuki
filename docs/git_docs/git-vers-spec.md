> **Status (2026-11-17):** The in-repo channel skeleton is `src-tauri/src/channel` (`channel-direct` / `channel-mas` / `channel-msstore`). The app already ships **2.0.x-beta** on the direct channel, so the “no release yet” framing below is stale. This file is still the only write-up of the store and license contract, and it has not been re-audited against the module. Do not treat it as as-built.

Below is the channel skeleton: Rust, capabilities, configs, frontend, scripts, CI, and the license server contract.

## 0. Principles everything rests on

1. **The channel is selected by a cargo feature, the platform via `target_os`.** Invalid combinations (`mas` not on macOS, `msstore` not on Windows) don't build.
2. **Channel code lives only in `channel/`.** Outside of it there's not a single `#[cfg(feature = "channel-…")]`, except for registering the updater plugin.
3. **Store binaries physically do not contain** the updater, the network client for licenses, or crypto for tokens. This is verified in CI.
4. **Rust is the source of truth**; the frontend asks it for `channel_info`. The build-time constant `__CHANNEL__` is only needed to strip the license UI from the JS bundle of store versions.

## 1. Structure

```
dither-yuki/
├── crates/engine/                  # no knowledge of channels
├── src-tauri/
│   ├── Cargo.toml
│   ├── tauri.conf.json             # base, nothing channel-specific
│   ├── tauri.macos.conf.json       # (if needed) picked up automatically
│   ├── tauri.windows.conf.json
│   ├── capabilities/
│   │   ├── main.json               # common
│   │   └── updater.json            # direct only
│   ├── channels/
│   │   ├── direct.conf.json
│   │   ├── mas.conf.json
│   │   └── msstore.conf.json
│   ├── entitlements/
│   │   └── mas.entitlements
│   ├── keys/license_pub.bin        # Ed25519 public key (32 bytes)
│   └── src/
│       ├── lib.rs
│       ├── channel/{mod,commands,direct,mas,msstore}.rs
│       └── platform/{mod,macos,windows}.rs
├── scripts/tauri.mjs               # single entry point for dev/build
├── scripts/package-msix.ps1
├── src/channel/{index.ts,useEntitlement.ts}
└── .github/{actions/setup,workflows/ci.yml,workflows/release.yml}
```

## 2. Cargo.toml

```toml
[features]
default = []
channel-direct = [
  "dep:tauri-plugin-updater", "dep:reqwest",
  "dep:ed25519-dalek", "dep:base64", "dep:machine-uid",
]
channel-mas     = []
channel-msstore = []

[dependencies]
async-trait = "0.1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
tauri-plugin-updater = { version = "2", optional = true }
reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls"], optional = true }
ed25519-dalek = { version = "2", optional = true }
base64 = { version = "0.22", optional = true }
machine-uid = { version = "0.5", optional = true }
```

Side effect: store versions pull in neither `reqwest` nor crypto for tokens, which means no extra network code for Apple's review.

## 3. `channel/mod.rs`

```rust
use std::sync::Arc;
use async_trait::async_trait;
use serde::Serialize;

// ── Invalid configurations don't build ────────────────────────────────
#[cfg(not(any(feature = "channel-direct", feature = "channel-mas", feature = "channel-msstore")))]
compile_error!("Pick a channel: channel-direct | channel-mas | channel-msstore");

#[cfg(any(
    all(feature = "channel-direct", feature = "channel-mas"),
    all(feature = "channel-direct", feature = "channel-msstore"),
    all(feature = "channel-mas", feature = "channel-msstore"),
))]
compile_error!("Channels are mutually exclusive: exactly one is required");

#[cfg(all(feature = "channel-mas", not(target_os = "macos")))]
compile_error!("channel-mas builds only on macOS");

#[cfg(all(feature = "channel-msstore", not(target_os = "windows")))]
compile_error!("channel-msstore builds only on Windows");

pub mod commands;
#[cfg(feature = "channel-direct")]  mod direct;
#[cfg(feature = "channel-mas")]     mod mas;
#[cfg(feature = "channel-msstore")] mod msstore;

// ── Types common to all channels ──────────────────────────────────────
#[derive(Serialize, Clone, Copy, Debug)]
#[serde(rename_all = "snake_case")]
pub enum LockReason { NoLicense, Expired, WrongDevice, Invalid }

#[derive(Serialize, Clone, Debug)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Entitlement {
    Unlocked,
    Locked { reason: LockReason },
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ChannelInfo {
    pub name: &'static str,          // "direct" | "mas" | "msstore"
    pub can_activate: bool,          // show the key entry
    pub can_self_update: bool,       // show the update check
    pub shows_external_purchase: bool, // "buy on the site" links (forbidden in stores)
}

#[derive(Serialize, Clone)]
pub struct UpdateInfo { pub version: String, pub notes: Option<String> }

#[derive(Debug, thiserror::Error)]
pub enum ChannelError {
    #[error("not supported in this distribution channel")]
    Unsupported,
    #[error("{0}")]
    Failed(String),
}

#[async_trait]
pub trait Channel: Send + Sync {
    fn info(&self) -> ChannelInfo;
    fn entitlement(&self) -> Entitlement;

    async fn activate(&self, _key: String) -> Result<Entitlement, ChannelError> {
        Err(ChannelError::Unsupported)
    }
    async fn deactivate(&self) -> Result<(), ChannelError> {
        Err(ChannelError::Unsupported)
    }
    async fn refresh(&self) -> Result<Entitlement, ChannelError> {
        Ok(self.entitlement())
    }
    async fn check_update(&self, _app: tauri::AppHandle) -> Result<Option<UpdateInfo>, ChannelError> {
        Ok(None)
    }
    async fn install_update(&self, _app: tauri::AppHandle) -> Result<(), ChannelError> {
        Err(ChannelError::Unsupported)
    }
}

pub type SharedChannel = Arc<dyn Channel>;

#[cfg(feature = "channel-direct")]
pub fn current(app: &tauri::AppHandle) -> SharedChannel { Arc::new(direct::DirectChannel::new(app)) }
#[cfg(feature = "channel-mas")]
pub fn current(_: &tauri::AppHandle) -> SharedChannel { Arc::new(mas::MasChannel) }
#[cfg(feature = "channel-msstore")]
pub fn current(_: &tauri::AppHandle) -> SharedChannel { Arc::new(msstore::MsStoreChannel) }
```

## 4. `channel/commands.rs`

The commands always exist, so `generate_handler!` doesn't need `cfg`. In stores they simply return `Unsupported`.

```rust
use super::*;
use tauri::State;

#[tauri::command]
pub fn channel_info(ch: State<'_, SharedChannel>) -> ChannelInfo { ch.info() }

#[tauri::command]
pub fn entitlement(ch: State<'_, SharedChannel>) -> Entitlement { ch.entitlement() }

#[tauri::command]
pub async fn activate_license(ch: State<'_, SharedChannel>, key: String) -> Result<Entitlement, String> {
    ch.activate(key).await.map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn deactivate_license(ch: State<'_, SharedChannel>) -> Result<(), String> {
    ch.deactivate().await.map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn refresh_license(ch: State<'_, SharedChannel>) -> Result<Entitlement, String> {
    ch.refresh().await.map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn check_update(ch: State<'_, SharedChannel>, app: tauri::AppHandle) -> Result<Option<UpdateInfo>, String> {
    ch.check_update(app).await.map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn install_update(ch: State<'_, SharedChannel>, app: tauri::AppHandle) -> Result<(), String> {
    ch.install_update(app).await.map_err(|e| e.to_string())
}
```

## 5. `channel/direct.rs`: license and updates

Token model: `base64url(payload_json) . base64url(ed25519_signature)`.

```rust
use super::*;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::Deserialize;
use std::{path::PathBuf, sync::Mutex, time::{SystemTime, UNIX_EPOCH}};
use tauri::Manager;
use tauri_plugin_updater::UpdaterExt;

const PUBLIC_KEY: &[u8; 32] = include_bytes!("../../keys/license_pub.bin");
const API: &str = match option_env!("YUKI_LICENSE_API") {
    Some(v) => v, None => "https://license.example.com",
};

#[derive(Deserialize)]
struct Claims {
    lid: String,      // license id
    device: String,   // device binding
    exp: i64,         // "soft" expiry: time to refresh the token online
    hard_exp: i64,    // "hard" expiry: after this, lock out
}

pub struct DirectChannel {
    path: PathBuf,
    device: String,
    http: reqwest::Client,
    token: Mutex<Option<String>>,
}

impl DirectChannel {
    pub fn new(app: &tauri::AppHandle) -> Self {
        let dir = app.path().app_data_dir().expect("app data dir");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("license.token");
        let token = std::fs::read_to_string(&path).ok().map(|s| s.trim().to_owned());
        Self {
            path,
            device: machine_uid::get().unwrap_or_else(|_| "unknown".into()),
            http: reqwest::Client::builder().timeout(std::time::Duration::from_secs(15)).build().unwrap(),
            token: Mutex::new(token),
        }
    }

    fn now() -> i64 {
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64
    }

    fn evaluate(&self, token: &str) -> Entitlement {
        match verify(token, &self.device, Self::now()) {
            Ok(_) => Entitlement::Unlocked,
            Err(reason) => Entitlement::Locked { reason },
        }
    }

    fn persist(&self, token: Option<&str>) {
        *self.token.lock().unwrap() = token.map(str::to_owned);
        match token {
            Some(t) => { // atomic write
                let tmp = self.path.with_extension("tmp");
                if std::fs::write(&tmp, t).is_ok() { let _ = std::fs::rename(&tmp, &self.path); }
            }
            None => { let _ = std::fs::remove_file(&self.path); }
        }
    }

    async fn post(&self, route: &str, body: serde_json::Value) -> Result<String, ChannelError> {
        #[derive(Deserialize)] struct Resp { token: String }
        let r = self.http.post(format!("{API}{route}")).json(&body).send().await
            .map_err(|e| ChannelError::Failed(format!("network: {e}")))?;
        if !r.status().is_success() {
            let status = r.status();
            let msg = r.text().await.unwrap_or_default();
            return Err(ChannelError::Failed(format!("{status}: {msg}")));
        }
        Ok(r.json::<Resp>().await.map_err(|e| ChannelError::Failed(e.to_string()))?.token)
    }
}

fn verify(token: &str, device: &str, now: i64) -> Result<Claims, LockReason> {
    let (p, s) = token.split_once('.').ok_or(LockReason::Invalid)?;
    let payload = B64.decode(p).map_err(|_| LockReason::Invalid)?;
    let sig = Signature::from_slice(&B64.decode(s).map_err(|_| LockReason::Invalid)?)
        .map_err(|_| LockReason::Invalid)?;
    VerifyingKey::from_bytes(PUBLIC_KEY).map_err(|_| LockReason::Invalid)?
        .verify_strict(&payload, &sig).map_err(|_| LockReason::Invalid)?;
    let c: Claims = serde_json::from_slice(&payload).map_err(|_| LockReason::Invalid)?;
    if c.device != device { return Err(LockReason::WrongDevice); }
    if now >= c.hard_exp  { return Err(LockReason::Expired); }
    Ok(c)
}

#[async_trait]
impl Channel for DirectChannel {
    fn info(&self) -> ChannelInfo {
        ChannelInfo { name: "direct", can_activate: true, can_self_update: true, shows_external_purchase: true }
    }

    fn entitlement(&self) -> Entitlement {
        match self.token.lock().unwrap().as_deref() {
            None => Entitlement::Locked { reason: LockReason::NoLicense },
            Some(t) => self.evaluate(t),
        }
    }

    async fn activate(&self, key: String) -> Result<Entitlement, ChannelError> {
        let t = self.post("/v1/activate", serde_json::json!({
            "key": key, "device": self.device, "app_version": env!("CARGO_PKG_VERSION"),
        })).await?;
        let e = self.evaluate(&t);
        if matches!(e, Entitlement::Unlocked) { self.persist(Some(&t)); }
        Ok(e)
    }

    async fn deactivate(&self) -> Result<(), ChannelError> {
        let t = self.token.lock().unwrap().clone();
        if let Some(t) = t {
            // best effort: release the slot on the server; locally we clear it regardless
            let _ = self.post("/v1/deactivate", serde_json::json!({ "token": t, "device": self.device })).await;
        }
        self.persist(None);
        Ok(())
    }

    async fn refresh(&self) -> Result<Entitlement, ChannelError> {
        let Some(t) = self.token.lock().unwrap().clone() else { return Ok(self.entitlement()) };
        let soft_expired = verify(&t, &self.device, Self::now() + 7 * 86400).is_err(); // refresh 7 days ahead
        if !soft_expired { return Ok(self.entitlement()); }
        match self.post("/v1/refresh", serde_json::json!({ "token": t, "device": self.device })).await {
            Ok(new) if matches!(self.evaluate(&new), Entitlement::Unlocked) => { self.persist(Some(&new)); }
            Ok(_) | Err(_) => {} // no network or server unreachable: stay within grace
        }
        Ok(self.entitlement())
    }

    async fn check_update(&self, app: tauri::AppHandle) -> Result<Option<UpdateInfo>, ChannelError> {
        let u = app.updater().map_err(|e| ChannelError::Failed(e.to_string()))?
            .check().await.map_err(|e| ChannelError::Failed(e.to_string()))?;
        Ok(u.map(|u| UpdateInfo { version: u.version, notes: u.body }))
    }

    async fn install_update(&self, app: tauri::AppHandle) -> Result<(), ChannelError> {
        let Some(u) = app.updater().map_err(|e| ChannelError::Failed(e.to_string()))?
            .check().await.map_err(|e| ChannelError::Failed(e.to_string()))? else { return Ok(()) };
        u.download_and_install(|_, _| {}, || {}).await.map_err(|e| ChannelError::Failed(e.to_string()))
    }
}
```

What is deliberately not included and is worth adding later: protection against system clock rollback (store `last_seen` and compare), explicit revocation on refund (the server returns 403 on `/refresh`, and then `persist(None)`), and device binding via a more robust identifier than `machine-uid`.

## 6. Store stubs

```rust
// mas.rs
use super::*;
pub struct MasChannel;
#[async_trait]
impl Channel for MasChannel {
    fn info(&self) -> ChannelInfo {
        ChannelInfo { name: "mas", can_activate: false, can_self_update: false, shows_external_purchase: false }
    }
    fn entitlement(&self) -> Entitlement { Entitlement::Unlocked } // purchased means installed
}
```

`msstore.rs` is analogous (`name: "msstore"`). If a trial or IAP appears later, all of it is added only in these files.

## 7. `lib.rs`

```rust
mod channel;
mod platform;

pub fn run() {
    let builder = tauri::Builder::default();

    #[cfg(feature = "channel-direct")]
    let builder = builder.plugin(tauri_plugin_updater::Builder::new().build());

    builder
        .setup(|app| {
            use tauri::Manager;
            app.manage(channel::current(app.handle()));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            channel::commands::channel_info,
            channel::commands::entitlement,
            channel::commands::activate_license,
            channel::commands::deactivate_license,
            channel::commands::refresh_license,
            channel::commands::check_update,
            channel::commands::install_update,
            // …your engine commands
        ])
        .run(tauri::generate_context!())
        .expect("error while running Dither Yuki");
}
```

## 8. Capabilities: an important nuance

If `updater:default` sits in the shared capability while the plugin isn't registered in the store build, the build fails on the permission resolution. So the updater is moved into a separate capability that is included only in `direct`.

`capabilities/main.json`: common, without the updater. `capabilities/updater.json`:

```json
{
  "identifier": "updater",
  "windows": ["main"],
  "permissions": ["updater:default"]
}
```

In the base `tauri.conf.json`: `"app": { "security": { "capabilities": ["main"] } }`.

## 9. Channel overlays

Tauri merges configs as a JSON Merge Patch, so **arrays are replaced entirely**, not appended. Hence the full list of capabilities in the overlay.

`channels/direct.conf.json`:
```json
{
  "app": { "security": { "capabilities": ["main", "updater"] } },
  "bundle": { "createUpdaterArtifacts": true },
  "plugins": {
    "updater": {
      "pubkey": "<Tauri updater public key>",
      "endpoints": ["https://updates.vasdas.net/ditheryuki/{{target}}/{{current_version}}/latest.json"]
    }
  }
}
```

`channels/mas.conf.json`:
```json
{
  "bundle": {
    "macOS": {
      "entitlements": "entitlements/mas.entitlements",
      "files": { "embedded.provisionprofile": "certs/DitherYuki_MAS.provisionprofile" }
    }
  }
}
```

`entitlements/mas.entitlements` (minimum):
```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>com.apple.security.app-sandbox</key><true/>
  <key>com.apple.security.files.user-selected.read-write</key><true/>
  <key>com.apple.application-identifier</key><string>TEAMID.com.vasdas.ditheryuki</string>
  <key>com.apple.developer.team-identifier</key><string>TEAMID</string>
</dict></plist>
```

The network entitlement (`network.client`) isn't needed in the store version, since it has no network. This simplifies review. Verify the exact set of keys and the provisioning profile format against the App Store section of Tauri's documentation.

`channels/msstore.conf.json` is almost empty: the build goes through `--no-bundle` and is then packaged by the script (section 12).

## 10. Frontend

`vite.config.ts`:
```ts
define: { __CHANNEL__: JSON.stringify(process.env.VITE_CHANNEL) }
```

`src/channel/index.ts`:
```ts
import { invoke } from '@tauri-apps/api/core';

declare const __CHANNEL__: 'direct' | 'mas' | 'msstore';
export type Entitlement =
  | { state: 'unlocked' }
  | { state: 'locked'; reason: 'no_license' | 'expired' | 'wrong_device' | 'invalid' };
export type ChannelInfo = {
  name: typeof __CHANNEL__; canActivate: boolean; canSelfUpdate: boolean; showsExternalPurchase: boolean;
};

export const IS_DIRECT = __CHANNEL__ === 'direct';

export async function bootChannel(): Promise<ChannelInfo> {
  const info = await invoke<ChannelInfo>('channel_info');
  if (info.name !== __CHANNEL__) throw new Error(`Channel mismatch: rust=${info.name}, web=${__CHANNEL__}`);
  return info;
}
```

The license UI is wired up so that Vite strips it from the store bundles:

```tsx
const LicenseDialog = IS_DIRECT ? lazy(() => import('./license/LicenseDialog')) : null;
const UpdateBanner  = IS_DIRECT ? lazy(() => import('./update/UpdateBanner'))  : null;
```

Exception: when checking `info.name !== __CHANNEL__`, it's better to show a clear build error rather than an empty screen.

## 11. Scripts

`scripts/tauri.mjs` — the single entry point so that flags don't spread across `package.json`:

```js
import { spawnSync } from 'node:child_process';

const [mode, channel] = process.argv.slice(2);           // build|dev  direct|mas|msstore
const plat = process.platform;                           // darwin | win32
const ok = { direct: ['darwin', 'win32'], mas: ['darwin'], msstore: ['win32'] };
if (!['build', 'dev'].includes(mode) || !ok[channel]?.includes(plat)) {
  console.error(`Not allowed: ${mode} ${channel} on ${plat}`); process.exit(1);
}

const args = ['tauri', mode, '--features', `channel-${channel}`,
              '--config', `src-tauri/channels/${channel}.conf.json`];
if (mode === 'build') {
  if (plat === 'darwin') args.push('--target', 'universal-apple-darwin');
  if (channel === 'mas') args.push('--bundles', 'app');
  if (channel === 'msstore') args.push('--no-bundle');
}
const r = spawnSync('npx', args, {
  stdio: 'inherit', shell: plat === 'win32',
  env: { ...process.env, VITE_CHANNEL: channel },
});
process.exit(r.status ?? 1);
```

`package.json`:
```json
"scripts": {
  "dev":              "node scripts/tauri.mjs dev direct",
  "build:direct":     "node scripts/tauri.mjs build direct",
  "build:mas":        "node scripts/tauri.mjs build mas",
  "build:msstore":    "node scripts/tauri.mjs build msstore"
}
```

For rust-analyzer: `"rust-analyzer.cargo.features": ["channel-direct"]` in the editor settings, otherwise `compile_error!` will highlight the project in red.

## 12. Store artifacts

**Mac App Store**: after `build:mas` you need to package the `.app` into a `.pkg` and sign it with the installer certificate:
```bash
xcrun productbuild --sign "<Installer identity>" \
  --component "src-tauri/target/universal-apple-darwin/release/bundle/macos/Dither Yuki.app" /Applications \
  DitherYuki.pkg
```
Upload goes through Transporter. Take the certificate names (Apple Distribution and the installer certificate) from your account.

**Microsoft Store** is the least-documented path for Tauri, and here I recommend doing a short experiment first. The scheme: `--no-bundle` produces `target/release/dither-yuki.exe`, then `scripts/package-msix.ps1` builds the package via `makeappx pack` (you need `AppxManifest.xml`, icons of the required sizes, and the exe with resources). The Store signs the package itself on submission. Main risks: the WebView2 dependency (check how this is handled in MSIX variants of Tauri apps on the current version) and file permissions inside the MSIX container. Budget a separate small spike for this before everything else on Windows.

## 13. CI

`.github/workflows/ci.yml` (on every PR):

```yaml
name: ci
on: [pull_request, push]
jobs:
  check:
    strategy:
      matrix:
        include:
          - { os: macos-latest,   channel: direct }
          - { os: macos-latest,   channel: mas }
          - { os: windows-latest, channel: direct }
          - { os: windows-latest, channel: msstore }
    runs-on: ${{ matrix.os }}
    defaults: { run: { working-directory: src-tauri } }
    steps:
      - uses: actions/checkout@v4
      - uses: ./.github/actions/setup
      - run: cargo check --features channel-${{ matrix.channel }}
      - run: cargo test  --features channel-${{ matrix.channel }}
      - name: Store binaries contain no direct dependencies
        if: matrix.channel != 'direct'
        shell: bash
        run: |
          ! cargo tree -e normal --features channel-${{ matrix.channel }} \
            | grep -E "tauri-plugin-updater|ed25519-dalek|machine-uid"
```

`release.yml` (on a `v*` tag) uses the same `setup` and a matrix of four jobs, where each runs `npm run build:<channel>` and uploads the artifact. Bind secrets to each channel's `environment` so they don't overlap:

| Job | Secrets |
|---|---|
| mac-direct | Developer ID Application certificate, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD` (app-specific), `APPLE_TEAM_ID`, `TAURI_SIGNING_PRIVATE_KEY` (+ password) |
| mac-mas | Apple Distribution + installer certificate, provisioning profile |
| win-direct | Azure Trusted Signing credentials (or a signing command) and `TAURI_SIGNING_PRIVATE_KEY` |
| win-msstore | no signing needed (the Store signs), self-signed for local testing |

For now, leave store publishing manual; add direct releases (artifact upload and `latest.json` update) as the last step of the release workflow.

## 14. License server (separate repository)

The contract that `DirectChannel` expects:

- `POST /v1/activate` `{key, device, app_version}` → `{token}`; validates the key, activation limit (e.g., 2), records the device
- `POST /v1/refresh` `{token, device}` → `{token}`; 403 for refunded or revoked
- `POST /v1/deactivate` `{token, device}` → 204; frees the slot
- `POST /webhook/<provider>`: payment → create license → email with the key

Token: `payload = {lid, device, exp: now+30d, hard_exp: now+45d}`, Ed25519 signature, private key only in the server's secrets, public key lives in the app's `keys/license_pub.bin`. The token format is used in both places, so it's convenient to put its definition in a tiny shared crate so it doesn't drift.

## 15. Order of work in a single branch

1. **Skeleton**: features, `compile_error!`, `channel/`, capabilities, `scripts/tauri.mjs`, `bootChannel` and UI gating, `ci.yml`. Done when all four combinations pass `cargo check` and the dependency check is green.
2. **Direct**: `DirectChannel` + server + updater + signing and notarization. Done when a signed build installs on a clean Mac and Windows machine, activates, updates to the next version, and locks after revocation.
3. **MAS**: sandbox, entitlements, `.pkg`, upload to TestFlight. The main check: opening and saving files and all image work inside the sandbox.
4. **MS Store**: the MSIX and WebView2 spike, then certification in Partner Center.

Two notes on store review. In `mas` and `msstore`, no UI should mention buying on the site or other channels (that's why `ChannelInfo` has `shows_external_purchase`, and check this flag everywhere there are links to the site). And in store versions it's better not to show messages about "license" and "updates", since they don't exist there.

If you want, as the next step I can write out `release.yml` in full, `package-msix.ps1` with the manifest, or the license server code in axum.