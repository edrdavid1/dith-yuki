//! Adapter fingerprint for GPU family self-test cache keys.

use std::sync::OnceLock;

use crate::blocked_fs::BLOCKED_FS_SHADER_SRC;
use crate::blue_noise_gpu::BLUE_NOISE_SHADER_SRC;

/// Stable fields from `wgpu::AdapterInfo` used as a self-test cache key.
#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct AdapterFingerprint {
    pub name: String,
    pub backend: String,
    pub device_type: String,
    pub driver: String,
    pub driver_info: String,
    pub vendor: u32,
    pub device: u32,
}

impl AdapterFingerprint {
    pub fn from_wgpu(info: &wgpu::AdapterInfo) -> Self {
        Self {
            name: info.name.clone(),
            backend: format!("{:?}", info.backend),
            device_type: format!("{:?}", info.device_type),
            driver: info.driver.clone(),
            driver_info: info.driver_info.clone(),
            vendor: info.vendor,
            device: info.device,
        }
    }

    pub fn is_software(&self) -> bool {
        self.device_type.eq_ignore_ascii_case("Cpu")
            || self.name.to_ascii_lowercase().contains("llvmpipe")
            || self.name.to_ascii_lowercase().contains("swiftshader")
            || self.driver.to_ascii_lowercase().contains("warp")
    }
}

/// Blake3 of blocked_fs + blue_noise WGSL sources (cache invalidates on shader edit).
pub fn gpu_family_shader_hash() -> String {
    static HASH: OnceLock<String> = OnceLock::new();
    HASH.get_or_init(|| {
        let mut hasher = blake3::Hasher::new();
        hasher.update(BLOCKED_FS_SHADER_SRC.as_bytes());
        hasher.update(b"\n---\n");
        hasher.update(BLUE_NOISE_SHADER_SRC.as_bytes());
        hasher.finalize().to_hex().to_string()
    })
    .clone()
}

/// Keep in sync with engine-gpu Cargo.toml `wgpu = "24.0"`.
pub fn wgpu_version_label() -> &'static str {
    "24.0"
}

/// Allow `DeviceType::Cpu` adapters (llvmpipe / WARP) — debug/tests only.
pub fn allow_software_adapter() -> bool {
    if !cfg!(debug_assertions) && !cfg!(test) {
        return false;
    }
    matches!(
        std::env::var("DITHER_ALLOW_SOFTWARE_ADAPTER")
            .unwrap_or_default()
            .as_str(),
        "1" | "true" | "yes"
    )
}
