fn main() {
    println!("cargo:rustc-check-cfg=cfg(mobile)");
    println!("cargo:rerun-if-changed=../dist-web-admin");
    #[cfg(feature = "gui")]
    tauri_build::build();
}
