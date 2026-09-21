//! Shared project/pattern archive serialization (Track E0 + Track F).
//!
//! - [`archive`] — zip **write** helpers (read path is [`secure_zip`])
//! - [`secure_zip`] — sole ZIP read path with limits / allowlist (SPEC §6)
//! - [`limits`] — archive resource ceilings
//! - [`assets`] — threshold-map content hashing, embed paths, synthetic materialize
//! - [`migrate`] — per-kind `format_version` gates (`dyproj` vs `dyuki`)
//! - [`pattern`] — `.dyuki` pack/unpack (placeholders, app_version_min)

pub mod archive;
pub mod assets;
pub mod document_dto;
pub mod features;
pub mod id_remap;
pub mod limits;
pub mod manifest;
pub mod migrate;
pub mod pattern;
pub mod pixels;
pub mod project;
pub mod sanitize;
pub mod secure_json;
pub mod secure_zip;
pub mod share;

pub use archive::{
    detect_archive_kind_from_bytes, peek_mimetype, ArchiveError, ZipArchiveReader, ZipArchiveWriter,
    MIME_DYPROJ, MIME_DYPROJ_LEGACY, MIME_DYUKI, MIME_DYUKI_LEGACY,
};
pub use features::{
    feature_by_id, required_version_for_features, unknown_required_features, FeatureDef,
    FormatVersion, FEATURE_REGISTRY, SUPPORTED_FORMAT_MAJOR,
};
pub use limits::ArchiveLimits;
pub use manifest::{
    build_dyproj_manifest_json, build_dyuki_manifest_json, build_manifest_files, check_open_gate,
    normalize_manifest_value, verify_manifest_files, FileIntegrity, GeneratorInfo,
    ManifestDocumentInfo, ManifestFiles, NormalizedManifest,
};
pub use sanitize::{sanitize_display_string, sanitize_display_string_or, sanitize_filename};
pub use secure_json::{parse_value as parse_json_value, SecureJsonError, MAX_JSON_DEPTH};
pub use secure_zip::{ExpectedKind, SecureZipArchive, SecureZipError};
pub use assets::{
    asset_cache_root, content_hash, materialize_threshold_map, materialize_threshold_map_with_hash,
    parse_threshold_basename, sha256_hex, threshold_map_basename, threshold_map_zip_entry,
    threshold_maps_cache_dir, AssetsError, THRESHOLD_MAPS_PREFIX,
};
pub use document_dto::{filter_from_file, filter_to_file, DocumentFile, FilterInstanceFile};
pub use id_remap::{remap_document_file, IdRemapTables, RemappedDocument};
pub use migrate::{
    check_format_version, migrate_dyproj, migrate_dyuki, ArchiveKind, Manifest, ProjectError,
    SOFT_SIZE_WARN_BYTES, SUPPORTED_DYPROJ_VERSION, SUPPORTED_DYUKI_VERSION,
};
pub use pattern::{
    check_app_version_min, export_pattern_from_document, import_pattern_into_document,
    min_app_version_for_filters, pack_pattern_to_bytes, unpack_pattern_from_bytes,
    write_pattern_to_path, ImportPatternResult, PalettePayload, PatternExportMeta,
    PatternFilterFile, PatternManifest, UnpackedPattern,
};
pub use pixels::{
    assemble_layer_png, build_composite_png, build_composite_rgba8, build_thumbnail_png,
    decode_png_to_f32, decode_png_to_f32_with_limits, reencode_png_clean, soft_size_warning,
    threshold_map_png_limits, PngDecodeLimits,
};
pub use project::{
    open_project_from_bytes, open_project_from_path, read_png_file, save_project_to_bytes,
    save_project_to_path, OpenProjectResult, SaveProjectResult,
};
pub use share::{
    downgrade_project_to_bytes, plan_downgrade, scan_archive_for_privacy_leaks, share_project_to_bytes,
    write_project_to_bytes, LossReport, ProjectWriteOptions, ShareCopyOptions,
};
