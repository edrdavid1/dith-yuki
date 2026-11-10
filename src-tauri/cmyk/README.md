# CMYK / ICC profiles

Bundled FOGRA/PSO ICC profiles were removed from the product. Soft proof and
print export use **user-imported** ICC files only (stored under the app data
`icc/` directory).

CI still keeps a FOGRA51 fixture under
`crates/engine-color/tests/fixtures/` for transform parity tests — that file is
not packaged into the app binary.
