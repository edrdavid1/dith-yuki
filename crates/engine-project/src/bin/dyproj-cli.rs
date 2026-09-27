//! Minimal CLI around `open_project_from_bytes` for version-compat drills
//! and as the frozen «reference reader» CI artifact (`reference-reader-v1.0`).
//!
//! Usage: `dyproj-cli open <path.dyproj>`
//! Exit 0 on success; non-zero with a human-readable error on stderr.

use engine_project::serialize::{open_project_from_bytes, ProjectError};
use engine_project::types::DocumentId;
use engine_tiles::TileCache;
use std::env;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let cmd = args.next().unwrap_or_default();
    match cmd.as_str() {
        "open" => {
            let Some(path) = args.next() else {
                eprintln!("usage: dyproj-cli open <path.dyproj>");
                return ExitCode::from(2);
            };
            match open_path(Path::new(&path)) {
                Ok(msg) => {
                    println!("{msg}");
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("{e}");
                    ExitCode::from(exit_code_for(&e))
                }
            }
        }
        "version" | "--version" | "-V" => {
            println!(
                "dyproj-cli {} (supported format {})",
                env!("CARGO_PKG_VERSION"),
                engine_project::serialize::SUPPORTED_FORMAT
            );
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("usage: dyproj-cli open <path.dyproj>");
            eprintln!("       dyproj-cli version");
            ExitCode::from(2)
        }
    }
}

fn open_path(path: &Path) -> Result<String, ProjectError> {
    let bytes = fs::read(path).map_err(|e| ProjectError::Io(e.to_string()))?;
    let staging = TileCache::new(64 * 1024 * 1024);
    let opened = open_project_from_bytes(&bytes, &staging, DocumentId::new(1))?;
    Ok(format!(
        "ok width={} height={} layers={}",
        opened.document.width,
        opened.document.height,
        opened.document.root.len()
    ))
}

fn exit_code_for(err: &ProjectError) -> u8 {
    match err {
        ProjectError::NeedsNewerApp { .. } | ProjectError::UnsupportedFeatures(_) => 3,
        ProjectError::UnsupportedVersion { .. } => 4,
        ProjectError::KindMismatch { .. } => 5,
        _ => 1,
    }
}
