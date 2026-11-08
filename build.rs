fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=packaging/icon.ico");
    // Embed the icon and file details in the .exe. Needs the Windows resource compiler, so only on a Windows host.
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("packaging/icon.ico")
            .set("ProductName", "Button Badger")
            .set("FileDescription", "Button Badger")
            .compile()
            .expect("embedding Windows resources");
    }
}
