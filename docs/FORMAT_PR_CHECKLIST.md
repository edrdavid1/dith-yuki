# Format change PR checklist

Use when a PR touches `.dyproj` / `.dyuki` on-disk shape, loaders, or writers.

## Before merge

- [ ] **Bump needed?** New incompatible field → `features_required` and/or
      `format` / `min_reader`. Prefer additive optional fields at format `1.x`.
- [ ] **Feature registry** updated in `serialize/features.rs` **and**
      `docs/FORMAT.md`.
- [ ] **Limits** changed only with a `FORMAT_DECISIONS.md` note; update
      `FORMAT.md` table.
- [ ] **Migration** path: legacy files without the new field still open.
- [ ] **Golden fixtures** in `tests/fixtures/**/v1/` **unchanged** (CI
      `SHA256SUMS`). Add *new* fixture names only with explicit regen flag.
- [ ] **Preservation** still green if unknown JSON / `ext/` must round-trip.
- [ ] **Security tests** still green (`security_zip`, `security_json_png`, …).
- [ ] **MIME / mimetype** still first Stored entry on write; legacy read OK.
- [ ] **Share Copy / privacy**: no absolute paths in JSON; author omitted by default.
- [ ] Docs: `FORMAT.md`, `SECURITY.md` / `FORMAT_DECISIONS.md` as needed.
- [ ] Schemas in `docs/schema/` updated if manifest/document shape changed.

## Do not

- Rewrite immutable golden bytes.
- Silently downgrade on ordinary Save (use explicit Export for older version).
- Accept executable / shader / plugin paths from archive JSON.
