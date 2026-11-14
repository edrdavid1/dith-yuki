use super::*;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine};
use ed25519_dalek::{Signature, VerifyingKey};
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::Manager;
use tauri_plugin_updater::UpdaterExt;

const PUBLIC_KEY: &[u8; 32] = include_bytes!("../../keys/license_pub.bin");
const API: &str = match option_env!("YUKI_LICENSE_API") {
    Some(v) => v,
    None => "https://license.example.com",
};

#[derive(Deserialize)]
struct Claims {
    lid: String,
    device: String,
    exp: i64,
    hard_exp: i64,
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
        let token = std::fs::read_to_string(&path)
            .ok()
            .map(|s| s.trim().to_owned());
        Self {
            path,
            device: machine_uid::get().unwrap_or_else(|_| "unknown".into()),
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .expect("reqwest client"),
            token: Mutex::new(token),
        }
    }

    fn now() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
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
            Some(t) => {
                let tmp = self.path.with_extension("tmp");
                if std::fs::write(&tmp, t).is_ok() {
                    let _ = std::fs::rename(&tmp, &self.path);
                }
            }
            None => {
                let _ = std::fs::remove_file(&self.path);
            }
        }
    }

    async fn post(&self, route: &str, body: serde_json::Value) -> Result<String, ChannelError> {
        #[derive(Deserialize)]
        struct Resp {
            token: String,
        }
        let r = self
            .http
            .post(format!("{API}{route}"))
            .json(&body)
            .send()
            .await
            .map_err(|e| ChannelError::Failed(format!("network: {e}")))?;
        if !r.status().is_success() {
            let status = r.status();
            let msg = r.text().await.unwrap_or_default();
            return Err(ChannelError::Failed(format!("{status}: {msg}")));
        }
        Ok(r.json::<Resp>()
            .await
            .map_err(|e| ChannelError::Failed(e.to_string()))?
            .token)
    }
}

fn verify(token: &str, device: &str, now: i64) -> Result<Claims, LockReason> {
    let (p, s) = token.split_once('.').ok_or(LockReason::Invalid)?;
    let payload = B64.decode(p).map_err(|_| LockReason::Invalid)?;
    let sig = Signature::from_slice(&B64.decode(s).map_err(|_| LockReason::Invalid)?)
        .map_err(|_| LockReason::Invalid)?;
    VerifyingKey::from_bytes(PUBLIC_KEY)
        .map_err(|_| LockReason::Invalid)?
        .verify_strict(&payload, &sig)
        .map_err(|_| LockReason::Invalid)?;
    let c: Claims = serde_json::from_slice(&payload).map_err(|_| LockReason::Invalid)?;
    if c.device != device {
        return Err(LockReason::WrongDevice);
    }
    if now >= c.hard_exp {
        return Err(LockReason::Expired);
    }
    Ok(c)
}

#[async_trait]
impl Channel for DirectChannel {
    fn info(&self) -> ChannelInfo {
        ChannelInfo {
            name: "direct",
            can_activate: true,
            can_self_update: true,
            shows_external_purchase: true,
        }
    }

    fn entitlement(&self) -> Entitlement {
        match self.token.lock().unwrap().as_deref() {
            None => Entitlement::Locked {
                reason: LockReason::NoLicense,
            },
            Some(t) => self.evaluate(t),
        }
    }

    async fn activate(&self, key: String) -> Result<Entitlement, ChannelError> {
        let t = self
            .post(
                "/v1/activate",
                serde_json::json!({
                    "key": key,
                    "device": self.device,
                    "app_version": env!("CARGO_PKG_VERSION"),
                }),
            )
            .await?;
        let e = self.evaluate(&t);
        if matches!(e, Entitlement::Unlocked) {
            self.persist(Some(&t));
        }
        Ok(e)
    }

    async fn deactivate(&self) -> Result<(), ChannelError> {
        let t = self.token.lock().unwrap().clone();
        if let Some(t) = t {
            let _ = self
                .post(
                    "/v1/deactivate",
                    serde_json::json!({ "token": t, "device": self.device }),
                )
                .await;
        }
        self.persist(None);
        Ok(())
    }

    async fn refresh(&self) -> Result<Entitlement, ChannelError> {
        let Some(t) = self.token.lock().unwrap().clone() else {
            return Ok(self.entitlement());
        };
        // Soft expiry: refresh when within 7 days of `exp` (or already past it).
        let soft_due = match verify(&t, &self.device, Self::now()) {
            Ok(c) => Self::now() + 7 * 86400 >= c.exp,
            Err(_) => true,
        };
        if !soft_due {
            return Ok(self.entitlement());
        }
        match self
            .post(
                "/v1/refresh",
                serde_json::json!({ "token": t, "device": self.device }),
            )
            .await
        {
            Ok(new) if matches!(self.evaluate(&new), Entitlement::Unlocked) => {
                self.persist(Some(&new));
            }
            Ok(_) | Err(_) => {}
        }
        Ok(self.entitlement())
    }

    async fn check_update(
        &self,
        app: tauri::AppHandle,
    ) -> Result<Option<UpdateInfo>, ChannelError> {
        let u = app
            .updater()
            .map_err(|e| ChannelError::Failed(e.to_string()))?
            .check()
            .await
            .map_err(|e| ChannelError::Failed(e.to_string()))?;
        Ok(u.map(|u| UpdateInfo {
            version: u.version,
            notes: u.body,
        }))
    }

    async fn install_update(&self, app: tauri::AppHandle) -> Result<(), ChannelError> {
        let Some(u) = app
            .updater()
            .map_err(|e| ChannelError::Failed(e.to_string()))?
            .check()
            .await
            .map_err(|e| ChannelError::Failed(e.to_string()))?
        else {
            return Ok(());
        };
        u.download_and_install(|_, _| {}, || {})
            .await
            .map_err(|e| ChannelError::Failed(e.to_string()))
    }
}
