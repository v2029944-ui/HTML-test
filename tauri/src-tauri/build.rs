fn main() {
    println!("cargo:rerun-if-env-changed=ANALIZ_UPDATER_PUBLIC_KEY");
    tauri_build::build();
}

