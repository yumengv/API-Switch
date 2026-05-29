fn main() {
    println!("cargo:rerun-if-changed=../dist-web-admin");
    #[cfg(feature = "gui")]
    tauri_build::build();
}
