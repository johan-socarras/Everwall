fn main() {
    // Own manifest so the process is per-monitor DPI aware from its first
    // instruction: the wallpaper API reports monitor sizes in the caller's
    // DPI view, and the images must be rendered at the real pixel size.
    let attributes = tauri_build::Attributes::new().windows_attributes(
        tauri_build::WindowsAttributes::new().app_manifest(include_str!("windows-app-manifest.xml")),
    );
    tauri_build::try_build(attributes).expect("failed to run tauri-build");
}
