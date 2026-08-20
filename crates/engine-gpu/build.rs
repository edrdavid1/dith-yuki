fn main() {
    println!("cargo:rerun-if-env-changed=DITHER_GPU");
    println!("cargo:rerun-if-env-changed=DITHER_FORCE_CPU");
}
