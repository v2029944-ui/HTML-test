fn main() {
    println!("cargo:rerun-if-env-changed=ANALIZ_UPDATER_PUBLIC_KEY");
    // Explorer resolves the relaunch display name through this localized PE string resource.
    let windows = tauri_build::WindowsAttributes::new()
        .append_rc_content("STRINGTABLE\nBEGIN\n  101 \"Анализ работ\"\nEND");
    tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows))
        .expect("Unable to build application resources");
}
