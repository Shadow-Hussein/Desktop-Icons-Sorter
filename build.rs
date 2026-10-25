// Embeds the app icon, the manifest (per-monitor DPI awareness, common
// controls v6 for TaskDialog) and version info into the exe.
fn main() {
    println!("cargo:rerun-if-changed=app.manifest");
    println!("cargo:rerun-if-changed=build/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon_with_id("build/icon.ico", "1");
    res.set_manifest_file("app.manifest");
    res.set("ProductName", "Desktop Icons Sorter");
    res.set("FileDescription", "Desktop Icons Sorter");
    res.set("LegalCopyright", "MIT License");
    res.compile().expect("compile the Windows resources");
}
