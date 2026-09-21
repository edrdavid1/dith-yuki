# Alpha: macOS system previews (Quick Look)

How to verify `.dyproj` / `.dyuki` **Space** preview during alpha (signing tier T1).
Finder icons stay the document-type `.icns` (no content thumbnails on icons).
Architecture: [`PREVIEWS.md`](./PREVIEWS.md).

## Sign (self-signed T1)

See [`SIGNING_ALPHA.md`](./SIGNING_ALPHA.md). Short version:

Release builds embed and sign **Preview** only
(`DitherQuickLookPreview.appex` in `Contents/PlugIns/`). After installing from
the DMG: Gatekeeper **Open Anyway**, then enable the Quick Look Preview
extension if macOS asks.

Local rebuild into an existing app:

```bash
bash scripts/macos-self-sign-cert.sh
export APPLE_SIGNING_IDENTITY="L'eco non di Bergamo"
scripts/build-quicklook.sh --tier alpha --app "/Applications/Dither Yuki.app"
```

Then: move to `/Applications`, **Open Anyway**, enable Quick Look extensions.

## Verify

```bash
pluginkit -mAvvv -p com.apple.quicklook.preview
# Force-enable if needed:
# pluginkit -e use -i com.dither.app.QuickLookPreview

qlmanage -p /path/to/sample.dyproj
```

In Finder: icons should be **proj/pattern type icons**; select a file and press
**Space** for the content preview. If an old Thumbnail extension is still
registered from alpha.10, remove that app / disable it and clear QL cache.

## Reset caches

```bash
qlmanage -r
qlmanage -r cache
killall Finder
# Icon cache (type icons):
killall Dock
```

## S0 spike (fixed image, no Rust)

```bash
# Requires xcodegen (brew install xcodegen)
scripts/build-quicklook.sh --tier dev --app "/Applications/Dither Yuki.app"
```

Then run the verify steps. S0 always returns the bundled magenta spike PNG so
registration/signing can be tested before `dither-thumb` is wired.

## Bug reports

Attach: macOS version, arch (Apple Silicon / Intel), whether the app was
downloaded (quarantine), `pluginkit` output for both extension points, and
whether Space / thumbnails fail independently.
