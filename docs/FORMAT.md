# Dither file format

Living reference for `.dyproj` / `.dyuki`. **Source of truth for limits and
features:** `crates/engine-project/src/serialize/{limits,features}.rs`.
This document mirrors those constants; if they diverge, trust the code and
update this file.

Canonical product spec: `.local-doc/SPEC_dither_file_format.md`.  
Decisions log: [`FORMAT_DECISIONS.md`](./FORMAT_DECISIONS.md).  
Threat model / rules: [`SECURITY.md`](./SECURITY.md).  
JSON Schema: [`schema/`](./schema/).

## MIME and extensions

| Extension | MIME (write) | Legacy MIME (read) |
|---|---|---|
| `.dyproj` | `application/vnd.dither.project+zip` | `application/x-dither-project` |
| `.dyuki` | `application/vnd.dither.pattern+zip` | `application/x-dither-pattern` |

Kind can also be detected from the first zip entry `mimetype` (Stored ASCII).

## Archive layout

### `.dyproj`

```
mimetype                 # first entry, Stored, no newline
manifest.json            # second among named payload (writers sort: manifest, then lex)
document.json
composite.png            # required on write (Raw-layer flat insurance)
thumbnail.png            # required on write, long side ≤ 512
layers/{id}.png
assets/threshold_maps/{blake3-32hex}.png
ext/...                  # opaque forward-compat blobs
```

### `.dyuki`

```
mimetype
manifest.json
filters.json
palettes.json
assets/threshold_maps/{blake3-32hex}.png
thumbnail.png            # optional (SHOULD); not written yet
```

### Deterministic write (Stage 4+)

- Entry order: `mimetype`, then `manifest.json`, then remaining names sorted.
- Fixed DOS mtime `1980-01-01 00:00:00`, unix mode `0o100644`.
- PNG / `mimetype` → Stored; JSON → Deflated; ZIP64 flag enabled.
- Saves use atomic replace (`engine_io::atomic_write`).

## Manifest

Writers emit both legacy and new fields:

| Field | Required on write | Notes |
|---|---|---|
| `format_version` | yes | Legacy alias = `format.major` (still `1`) |
| `kind` | yes | `dyproj` \| `dyuki` |
| `format` | yes | `{ major, minor }` |
| `min_reader` | yes | Open gate on `major` |
| `features_required` / `features_optional` | yes (may be `[]`) | |
| `generator` | yes | `{ app: "Dither", version }` only |
| `files` | yes (Stage 4+) | `path → { size, sha256 }`; omit on legacy |
| `document` / `width`/`height` | dyproj | |
| `name`, `app_version_min`, … | dyuki | `app_version_min` **ignored** for gates |

Open algorithm (summary): secure zip → parse manifest → kind → `min_reader` /
features → migrate → verify `files` hashes if present → load payload.

## App → format support

| App release | Max format major (open) | Notes |
|---|---|---|
| 0.2.x … 0.3.0-alpha.* (pre–Stage 2 writers) | 1 | Legacy `format_version` only |
| Current (Stage 2–7) | 1 | Reads legacy + `format`/`min_reader`/`files`; writes Stage 4 container |

## Feature registry

| id | since | required | description |
|---|---|---|---|
| `tiled-layers` | 2.0 | yes | Reserved; not written yet |

Writers emit the **minimum** `format` required by used features (today always `1.0`).

## Limits (from `ArchiveLimits`)

| Limit | `.dyproj` | `.dyuki` |
|---|---|---|
| Archive bytes | 8 GiB | 64 MiB |
| Entry count | 20 000 | 2 000 |
| Total uncompressed | 16 GiB | 256 MiB |
| `manifest.json` | 1 MiB | 1 MiB |
| Primary JSON | 64 MiB | 16 MiB |
| Single PNG entry | 1 GiB | 64 MiB |
| Compression ratio (non-PNG) | 200 | 200 |
| Path segments / name bytes | 4 / 200 | 4 / 200 |

PNG decode also caps pixels (16384² document; 4096² threshold maps).  
JSON depth default: `MAX_JSON_DEPTH` in `secure_json.rs`.

## Privacy / Share Copy

Ordinary Save never writes absolute paths (CustomPng → content-hash basename),
machine names, or undo history.

Explicit **Share Copy** (`share_project_to_bytes`): strip PNG metadata (default
on), omit author (default), no originals (none stored), optional compact JSON.

Explicit **Export for older version** (`downgrade_project_to_bytes`): never
runs on normal Save; today only format `1.x` with empty loss reports.

## Schemas and checklist

- JSON Schema drafts: [`docs/schema/`](./schema/)
- Format PR checklist: [`docs/FORMAT_PR_CHECKLIST.md`](./FORMAT_PR_CHECKLIST.md)
