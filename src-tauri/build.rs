fn main() {
    println!("cargo:rerun-if-changed=../dist-web-admin");
    tauri_build::build()
}
