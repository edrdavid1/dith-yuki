use std::sync::Arc;

use async_trait::async_trait;
use serde::Serialize;

// ── Invalid configurations don't build ────────────────────────────────
#[cfg(not(any(
    feature = "channel-direct",
    feature = "channel-mas",
    feature = "channel-msstore"
)))]
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
#[cfg(feature = "channel-direct")]
mod direct;
#[cfg(feature = "channel-mas")]
mod mas;
#[cfg(feature = "channel-msstore")]
mod msstore;

// ── Types common to all channels ──────────────────────────────────────
#[derive(Serialize, Clone, Copy, Debug)]
#[serde(rename_all = "snake_case")]
pub enum LockReason {
    NoLicense,
    Expired,
    WrongDevice,
    Invalid,
}

#[derive(Serialize, Clone, Debug)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Entitlement {
    Unlocked,
    Locked { reason: LockReason },
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ChannelInfo {
    pub name: &'static str,
    pub can_activate: bool,
    pub can_self_update: bool,
    pub shows_external_purchase: bool,
}

#[derive(Serialize, Clone)]
pub struct UpdateInfo {
    pub version: String,
    pub notes: Option<String>,
}

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
    async fn check_update(
        &self,
        _app: tauri::AppHandle,
    ) -> Result<Option<UpdateInfo>, ChannelError> {
        Ok(None)
    }
    async fn install_update(&self, _app: tauri::AppHandle) -> Result<(), ChannelError> {
        Err(ChannelError::Unsupported)
    }
}

pub type SharedChannel = Arc<dyn Channel>;

#[cfg(feature = "channel-direct")]
pub fn current(app: &tauri::AppHandle) -> SharedChannel {
    Arc::new(direct::DirectChannel::new(app))
}
#[cfg(feature = "channel-mas")]
pub fn current(_: &tauri::AppHandle) -> SharedChannel {
    Arc::new(mas::MasChannel)
}
#[cfg(feature = "channel-msstore")]
pub fn current(_: &tauri::AppHandle) -> SharedChannel {
    Arc::new(msstore::MsStoreChannel)
}
