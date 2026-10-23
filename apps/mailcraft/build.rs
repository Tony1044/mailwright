//! Windows only: embed the app icon and version information in `mailcraft.exe`, so it shows in
//! Explorer, the taskbar and Alt-Tab. Elsewhere this does nothing.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../assets/app-icon/mailcraft.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("../../assets/app-icon/mailcraft.ico")
        .set("ProductName", "MailCraft")
        .set("FileDescription", "MailCraft mail and calendar")
        .set("LegalCopyright", "Copyright (c) the MailCraft authors. MIT OR Apache-2.0.")
        .set("OriginalFilename", "mailcraft.exe")
        .set("InternalName", "mailcraft");
    if let Err(e) = res.compile() {
        println!("cargo:warning=mailcraft.exe built without icon/version resources: {e}");
    }
}
