# Fuzz targets for Dither archive loaders (SPEC §14.9)

Not a workspace member (cargo-fuzz profile conflicts). Run from repo root:

```bash
# once
rustup toolchain install nightly
cargo install cargo-fuzz

# seed corpus from goldens (corpus/ is gitignored — local only)
mkdir -p fuzz/corpus/fuzz_open_dyproj fuzz/corpus/fuzz_open_dyuki \
         fuzz/corpus/fuzz_manifest fuzz/corpus/fuzz_png_limits fuzz/corpus/fuzz_migrate
cp crates/engine-project/tests/fixtures/dyproj/v1/minimal.dyproj fuzz/corpus/fuzz_open_dyproj/
cp crates/engine-project/tests/fixtures/dyuki/v1/minimal.dyuki fuzz/corpus/fuzz_open_dyuki/

# 30 minutes per target (SPEC DoD)
cargo +nightly fuzz run fuzz_open_dyproj -- -max_total_time=1800
cargo +nightly fuzz run fuzz_open_dyuki -- -max_total_time=1800
cargo +nightly fuzz run fuzz_manifest -- -max_total_time=1800
cargo +nightly fuzz run fuzz_png_limits -- -max_total_time=1800
cargo +nightly fuzz run fuzz_migrate -- -max_total_time=1800
```

Any crash → copy the artifact into `crates/engine-project/tests/` as a
regression case that expects a typed error (never panic).

CI substitute (no nightly): `cargo test -p engine-project --test fuzz_mutation`.
