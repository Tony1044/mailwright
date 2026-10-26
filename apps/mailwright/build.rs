//! Windows only: embed the app icon and version information in `mailwright.exe`, so it shows in
//! Explorer, the taskbar and Alt-Tab. Elsewhere this does nothing.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../assets/app-icon/mailwright.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("../../assets/app-icon/mailwright.ico")
        .set("ProductName", "Mailwright")
        .set("FileDescription", "Mailwright mail and calendar")
        .set("LegalCopyright", "Copyright (c) the Mailwright authors. MIT OR Apache-2.0.")
        .set("OriginalFilename", "mailwright.exe")
        .set("InternalName", "mailwright");
    if let Err(e) = res.compile() {
        println!("cargo:warning=mailwright.exe built without icon/version resources: {e}");
    }
}
