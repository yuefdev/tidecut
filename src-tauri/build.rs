fn main() {
    tauri_build::build();

    // Tauri creates resource.lib in OUT_DIR and links it to app binaries.
    // Expose that directory so the cfg(test) link in lib.rs can give Windows
    // unit-test executables the same Common Controls v6 manifest.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        if let Ok(out_dir) = std::env::var("OUT_DIR") {
            println!("cargo:rustc-link-search=native={out_dir}");
        }
    }
}
