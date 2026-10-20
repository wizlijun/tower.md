fn main() {
    println!(
        "cargo:rustc-env=TOWERMD_TARGET={}",
        std::env::var("TARGET").expect("Cargo target")
    );
    tauri_build::build();
}
