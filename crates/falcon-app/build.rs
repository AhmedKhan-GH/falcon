//! On Windows, puts the icon and the version information into `falcon-app.exe`.
//! Nothing happens on other systems, and `winresource` is only a dependency on Windows.

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=assets/Falcon.ico");
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "windows") {
        // The file and product versions come from Cargo.toml.
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/Falcon.ico")
            .set("ProductName", "Falcon")
            .set("FileDescription", "Falcon")
            .set("CompanyName", "Ahmed Khan")
            .set("LegalCopyright", "Copyright (c) 2026 Ahmed Khan")
            .set("OriginalFilename", "falcon-app.exe");
        // Only the app: the test programs in this crate have no use for it.
        res.compile_for("falcon-app").expect("the resource compiler runs (it comes with the Windows SDK)");
    }
}
