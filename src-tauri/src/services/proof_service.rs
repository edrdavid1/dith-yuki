//! Soft-proof profile catalog (builtin + imported) and transform cache.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use engine_color::soft_proof::{
    ProofProfile, SoftProofConfig, SoftProofError, SoftProofIntent, SoftProofTransform,
    BUILTIN_FOGRA51_ICC, BUILTIN_FOGRA51_ID, BUILTIN_FOGRA52_ID,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

/// Soft limit for imported ICC files (spec suggestion).
pub const MAX_ICC_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct ProfileInfoDto {
    pub id: String,
    pub name: String,
    pub builtin: bool,
    pub has_perceptual: bool,
    pub has_relative: bool,
    pub has_absolute: bool,
}

#[derive(Debug, Error)]
pub enum ProofServiceError {
    #[error("{0}")]
    Message(String),
    #[error(transparent)]
    SoftProof(#[from] SoftProofError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl From<ProofServiceError> for String {
    fn from(e: ProofServiceError) -> Self {
        e.to_string()
    }
}

#[derive(Clone)]
struct StoredProfile {
    info: ProfileInfoDto,
    bytes: Arc<[u8]>,
}

pub struct ProofService {
    import_dir: PathBuf,
    profiles: Mutex<HashMap<String, StoredProfile>>,
    transforms: Mutex<HashMap<(String, SoftProofIntent, bool), Arc<SoftProofTransform>>>,
}

impl ProofService {
    pub fn new(app_data: &Path) -> Result<Self, ProofServiceError> {
        let import_dir = app_data.join("icc");
        fs::create_dir_all(&import_dir)?;
        let svc = Self {
            import_dir,
            profiles: Mutex::new(HashMap::new()),
            transforms: Mutex::new(HashMap::new()),
        };
        svc.reload_catalog()?;
        Ok(svc)
    }

    pub fn reload_catalog(&self) -> Result<(), ProofServiceError> {
        let mut map = HashMap::new();
        insert_builtin(
            &mut map,
            BUILTIN_FOGRA51_ID,
            "PSO Coated v3 (FOGRA51)",
            BUILTIN_FOGRA51_ICC,
        )?;

        if let Some((name, bytes)) = load_fogra52_icc() {
            let fogra51_hash = sha256_hex(BUILTIN_FOGRA51_ICC);
            let hash = sha256_hex(&bytes);
            if hash == fogra51_hash {
                log::warn!(
                    "FOGRA52 path resolved to the same bytes as FOGRA51; skipping builtin:fogra52 until a distinct Uncoated ICC is provided"
                );
            } else {
                insert_builtin(&mut map, BUILTIN_FOGRA52_ID, &name, &bytes)?;
            }
        }

        // Imported profiles: `{sha256}.icc` + sibling `{sha256}.json` metadata optional.
        if let Ok(entries) = fs::read_dir(&self.import_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("icc") {
                    continue;
                }
                let bytes = match fs::read(&path) {
                    Ok(b) => b,
                    Err(e) => {
                        log::warn!("skip icc {}: {e}", path.display());
                        continue;
                    }
                };
                if bytes.len() as u64 > MAX_ICC_BYTES {
                    continue;
                }
                let hash = sha256_hex(&bytes);
                let id = format!("import:{hash}");
                match ProofProfile::from_icc_bytes(&bytes) {
                    Ok(proof) => {
                        let name = read_import_display_name(&self.import_dir, &hash)
                            .unwrap_or_else(|| proof.info.description.clone());
                        map.insert(
                            id.clone(),
                            StoredProfile {
                                info: ProfileInfoDto {
                                    id,
                                    name,
                                    builtin: false,
                                    has_perceptual: proof.info.has_perceptual,
                                    has_relative: proof.info.has_relative,
                                    has_absolute: proof.info.has_absolute,
                                },
                                bytes: Arc::from(bytes),
                            },
                        );
                    }
                    Err(e) => log::warn!("skip invalid icc {}: {e}", path.display()),
                }
            }
        }

        *self.profiles.lock().expect("proof profiles") = map;
        self.transforms.lock().expect("proof transforms").clear();
        Ok(())
    }

    pub fn list_profiles(&self) -> Vec<ProfileInfoDto> {
        let map = self.profiles.lock().expect("proof profiles");
        let mut list: Vec<_> = map.values().map(|p| p.info.clone()).collect();
        list.sort_by(|a, b| match (a.builtin, b.builtin) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.name.cmp(&b.name),
        });
        list
    }

    pub fn import_profile(&self, path: &Path) -> Result<ProfileInfoDto, ProofServiceError> {
        let meta = fs::metadata(path)?;
        if meta.len() > MAX_ICC_BYTES {
            return Err(ProofServiceError::Message(format!(
                "ICC file too large ({} bytes; max {MAX_ICC_BYTES})",
                meta.len()
            )));
        }
        let bytes = fs::read(path)?;
        let proof = ProofProfile::from_icc_bytes(&bytes)?;
        let hash = sha256_hex(&bytes);
        let id = format!("import:{hash}");
        let dest = self.import_dir.join(format!("{hash}.icc"));
        fs::write(&dest, &bytes)?;
        write_import_display_name(&self.import_dir, &hash, &proof.info.description)?;

        let info = ProfileInfoDto {
            id: id.clone(),
            name: proof.info.description.clone(),
            builtin: false,
            has_perceptual: proof.info.has_perceptual,
            has_relative: proof.info.has_relative,
            has_absolute: proof.info.has_absolute,
        };
        self.profiles.lock().expect("proof profiles").insert(
            id,
            StoredProfile {
                info: info.clone(),
                bytes: Arc::from(bytes),
            },
        );
        Ok(info)
    }

    pub fn remove_profile(&self, id: &str) -> Result<(), ProofServiceError> {
        if id.starts_with("builtin:") {
            return Err(ProofServiceError::Message(
                "cannot remove a built-in profile".into(),
            ));
        }
        let hash = id
            .strip_prefix("import:")
            .ok_or_else(|| ProofServiceError::Message(format!("unknown profile id: {id}")))?;
        let icc = self.import_dir.join(format!("{hash}.icc"));
        let meta = self.import_dir.join(format!("{hash}.json"));
        let _ = fs::remove_file(icc);
        let _ = fs::remove_file(meta);
        self.profiles.lock().expect("proof profiles").remove(id);
        self.transforms
            .lock()
            .expect("proof transforms")
            .retain(|(pid, _, _), _| pid != id);
        Ok(())
    }

    pub fn has_profile(&self, id: &str) -> bool {
        self.profiles
            .lock()
            .expect("proof profiles")
            .contains_key(id)
    }

    /// ICC bytes for a catalog profile (print export embed + CMS).
    pub fn profile_icc_bytes(&self, id: &str) -> Option<Arc<[u8]>> {
        self.profiles
            .lock()
            .expect("proof profiles")
            .get(id)
            .map(|p| Arc::clone(&p.bytes))
    }

    /// If the configured profile is missing, disable proof (user re-enables after import).
    pub fn sanitize_config(&self, cfg: &mut SoftProofConfig) -> bool {
        if self.has_profile(&cfg.profile_id) {
            return false;
        }
        if cfg.profile_display_name.is_none() {
            cfg.profile_display_name =
                Some(SoftProofConfig::sanitize_display_name(&cfg.profile_id));
        }
        let changed = cfg.enabled;
        cfg.enabled = false;
        changed
    }

    pub fn transform_for(
        &self,
        cfg: &SoftProofConfig,
    ) -> Result<Option<Arc<SoftProofTransform>>, ProofServiceError> {
        if !cfg.enabled {
            return Ok(None);
        }
        let key = (cfg.profile_id.clone(), cfg.intent, cfg.bpc);
        {
            let cache = self.transforms.lock().expect("proof transforms");
            if let Some(t) = cache.get(&key) {
                return Ok(Some(Arc::clone(t)));
            }
        }
        let bytes = {
            let map = self.profiles.lock().expect("proof profiles");
            map.get(&cfg.profile_id)
                .map(|p| Arc::clone(&p.bytes))
                .ok_or_else(|| {
                    ProofServiceError::Message(format!(
                        "soft-proof profile not found: {}",
                        cfg.profile_id
                    ))
                })?
        };
        let build_t0 = std::time::Instant::now();
        let proof = ProofProfile::from_icc_bytes(&bytes)?;
        let xform = Arc::new(SoftProofTransform::new(&proof, cfg.intent, cfg.bpc)?);
        log::info!(
            target: "soft_proof",
            "transform_for profile={} intent={:?} bpc={} lut={} elapsed_ms={:.2}",
            cfg.profile_id,
            cfg.intent,
            cfg.bpc,
            xform.uses_lut(),
            build_t0.elapsed().as_secs_f64() * 1000.0
        );
        self.transforms
            .lock()
            .expect("proof transforms")
            .insert(key, Arc::clone(&xform));
        Ok(Some(xform))
    }

    /// After enable, background-build unused intents for the current profile (~ms each).
    pub fn warm_alternate_intents(self: &Arc<Self>, cfg: &SoftProofConfig) {
        let profile_id = cfg.profile_id.clone();
        let bpc = cfg.bpc;
        let current = cfg.intent;
        let bytes = {
            let map = self.profiles.lock().expect("proof profiles");
            map.get(&profile_id).map(|p| Arc::clone(&p.bytes))
        };
        let Some(bytes) = bytes else {
            return;
        };
        let svc = Arc::clone(self);
        std::thread::spawn(move || {
            let intents = [
                SoftProofIntent::Relative,
                SoftProofIntent::Perceptual,
                SoftProofIntent::Absolute,
            ];
            for intent in intents {
                if intent == current {
                    continue;
                }
                let key = (profile_id.clone(), intent, bpc);
                {
                    let cache = svc.transforms.lock().expect("proof transforms");
                    if cache.contains_key(&key) {
                        continue;
                    }
                }
                let t0 = std::time::Instant::now();
                if let Ok(proof) = ProofProfile::from_icc_bytes(&bytes) {
                    if let Ok(xform) = SoftProofTransform::new(&proof, intent, bpc) {
                        svc.transforms
                            .lock()
                            .expect("proof transforms")
                            .insert(key, Arc::new(xform));
                        log::debug!(
                            target: "soft_proof",
                            "warm intent={intent:?} elapsed_ms={:.2}",
                            t0.elapsed().as_secs_f64() * 1000.0
                        );
                    }
                }
            }
        });
    }
}

fn insert_builtin(
    map: &mut HashMap<String, StoredProfile>,
    id: &str,
    fallback_name: &str,
    bytes: &[u8],
) -> Result<(), ProofServiceError> {
    let proof = ProofProfile::from_icc_bytes(bytes)?;
    map.insert(
        id.to_string(),
        StoredProfile {
            info: ProfileInfoDto {
                id: id.to_string(),
                // Compact UI label (includes FOGRA tag); ICC desc kept for import path.
                name: fallback_name.to_string(),
                builtin: true,
                has_perceptual: proof.info.has_perceptual,
                has_relative: proof.info.has_relative,
                has_absolute: proof.info.has_absolute,
            },
            bytes: Arc::from(bytes.to_vec()),
        },
    );
    Ok(())
}

fn load_fogra52_icc() -> Option<(String, Vec<u8>)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("cmyk/pso-uncoated_v3_fogra52");
    if !dir.is_dir() {
        log::warn!("soft-proof: missing {}", dir.display());
        return None;
    }
    let preferred = [
        "PSOuncoated_v3.icc",
        "PSOuncoated_v3_fogra52.icc",
        "PSO_Uncoated_v3.icc",
        "PSOuncoated_v3_FOGRA52.icc",
    ];
    for name in preferred {
        let path = dir.join(name);
        if let Ok(bytes) = fs::read(&path) {
            if ProofProfile::from_icc_bytes(&bytes).is_ok() {
                return Some(("PSO Uncoated v3 (FOGRA52)".into(), bytes));
            }
        }
    }
    // Any CMYK ICC in the folder whose content is not identical to FOGRA51.
    let fogra51_hash = sha256_hex(BUILTIN_FOGRA51_ICC);
    let mut entries: Vec<_> = fs::read_dir(&dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .collect();
    entries.sort();
    for path in entries {
        if path.extension().and_then(|e| e.to_str()) != Some("icc") {
            continue;
        }
        let Ok(bytes) = fs::read(&path) else { continue };
        if sha256_hex(&bytes) == fogra51_hash {
            log::warn!(
                "soft-proof: {} is identical to PSO Coated v3 (FOGRA51); place the real PSOuncoated_v3.icc here for FOGRA52",
                path.display()
            );
            continue;
        }
        if let Ok(proof) = ProofProfile::from_icc_bytes(&bytes) {
            let name = if proof
                .info
                .description
                .to_ascii_lowercase()
                .contains("uncoated")
            {
                proof.info.description
            } else {
                format!("{} (FOGRA52)", proof.info.description)
            };
            return Some((name, bytes));
        }
    }
    None
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn read_import_display_name(dir: &Path, hash: &str) -> Option<String> {
    let path = dir.join(format!("{hash}.json"));
    let bytes = fs::read(path).ok()?;
    let v: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    v.get("name")?.as_str().map(|s| s.to_string())
}

fn write_import_display_name(dir: &Path, hash: &str, name: &str) -> Result<(), ProofServiceError> {
    let path = dir.join(format!("{hash}.json"));
    let v = serde_json::json!({ "name": name });
    fs::write(path, serde_json::to_vec_pretty(&v).unwrap())?;
    Ok(())
}
