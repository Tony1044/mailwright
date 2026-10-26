#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unreachable)]
//! Write the app icon as PNGs (for the .deb and the Windows .ico): `cargo run --example app_icons -- dir`.

fn main() {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "assets/app-icon".into()));
    let _ = std::fs::create_dir_all(&dir);
    for size in [16u32, 24, 32, 48, 64, 128, 256, 512] {
        let p = dir.join(format!("mailwright-{size}.png"));
        if let Err(e) = std::fs::write(&p, mailwright_ui::app_icon::png(size)) {
            eprintln!("{}: {e}", p.display());
        }
    }
}
