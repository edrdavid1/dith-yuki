# Alpha: macOS system previews (Quick Look)

How to verify `.dyproj` / `.dyuki` space-preview and Finder thumbnails during
alpha (signing tier T1). Architecture: [`PREVIEWS.md`](./PREVIEWS.md).

## Sign (self-signed T1)

See [`SIGNING_ALPHA.md`](./SIGNING_ALPHA.md). Short version:

```bash
bash scripts/macos-self-sign-cert.sh
export APPLE_SIGNING_IDENTITY="L'eco non di Bergamo"
scripts/build-quicklook.sh --tier alpha --app "/Applications/Dither Yuki.app"
```

Then: move to `/Applications`, **Open Anyway**, enable Quick Look extensions.

## Verify

```bash
pluginkit -mAvvv -p com.apple.quicklook.preview
pluginkit -mAvvv -p com.apple.quicklook.thumbnail
# Force-enable if needed:
# pluginkit -e use -i com.dither.app.QuickLookPreview
# pluginkit -e use -i com.dither.app.QuickLookThumbnail

qlmanage -p /path/to/sample.dyproj
qlmanage -t -s 512 -o /tmp/out /path/to/sample.dyproj
```

In Finder: select a `.dyproj` / `.dyuki`, press Space; check icon view thumbnails.

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
