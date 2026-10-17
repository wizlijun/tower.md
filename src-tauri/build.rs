fn main() {
    println!(
        "cargo:rustc-env=TOWERMD_HOST_TRIPLE={}",
        std::env::var("HOST").unwrap()
    );
    tauri_build::build();
}
