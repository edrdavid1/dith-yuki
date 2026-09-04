# FlexLayout (`flexlayout-react`) — LICENSE snapshot

**Purpose:** frozen copy for legal audit. Do not rely on a live npm/GitHub URL as the only record.

| Field | Value |
|-------|--------|
| Package | `flexlayout-react` |
| Version pinned in app | **0.7.15** (`frontend/package.json`) |
| Snapshot taken (UTC) | **2026-09-04T10:32:36Z** |
| npm `gitHead` (publish metadata) | `88cbbf55d005a94e9fc469c5ad7fd56fc363d31b` |
| npm tarball | `https://registry.npmjs.org/flexlayout-react/-/flexlayout-react-0.7.15.tgz` |
| npm `dist.integrity` (lockfile) | `sha512-ydTMdEoQO5BniylxVkSxa59rEY0+96lqqRII+QK+yq6028eHywPuxZawt4g45y5pMb9ptP4N9HPAQXAFsxwowQ==` |
| SHA-256 of `LICENSE` file | `59391baaf80d3af529e3f08adffc813fe54373e6365a2ae5ba3a991e9c25d16f` |
| Full text file in repo | [`flexlayout-LICENSE.MIT.txt`](./flexlayout-LICENSE.MIT.txt) |

## License field discrepancy (known)

| Source | Claims |
|--------|--------|
| `package.json` / npm metadata for 0.7.15 | `"license": "ISC"` |
| File `LICENSE` inside the published package (and this snapshot) | **MIT** (Copyright (c) 2017 Caplin Systems Ltd) |

**Source of truth for compliance:** the `LICENSE` file text (MIT), not the npm `license` field.  
ADR B2 already flagged this; this snapshot closes the “preserve a copy” action item.

## Re-verify later

If the pin bumps (e.g. to 0.10.x), take a **new** snapshot for that version: copy `LICENSE`, recompute SHA-256, record new `gitHead` / integrity.
