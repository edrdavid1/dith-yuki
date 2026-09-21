# Golden fixtures

Immutable sample archives for format compatibility tests.

| Path | Kind | Notes |
|---|---|---|
| `dyproj/v1/minimal.dyproj` | project v1 | 32×24 raster + Bayer4×4 |
| `dyuki/v1/minimal.dyuki` | pattern v1 | Floyd–Steinberg + 2-color palette |
| `SHA256SUMS` | lockfile | CI must fail if bytes change |

Do **not** regenerate existing files. To add a *new* fixture name:

```bash
GENERATE_GOLDEN_V1=1 cargo test -p engine-project --test golden_v1 generate_golden_v1_fixtures -- --ignored --nocapture
```

(`FORCE_GOLDEN_V1=1` overwrites — never use on locked names in CI.)
