# Test-only ICC fixtures

`PSOcoated_v3.icc` is **not** shipped in the app binary. Soft proof / print
export require a user-imported ICC at runtime.

These bytes exist only so CI can exercise CMYK transform parity without
redistributing a profile inside the product bundle.
