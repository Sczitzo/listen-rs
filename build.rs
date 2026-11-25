#[path = "src/mark.rs"]
mod mark;

use image::{codecs::png::PngEncoder, ExtendedColorType, ImageEncoder};
use std::{env, fs::File, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=src/mark.rs");
    let output = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let logo = File::create(output.join("listen-rust.png")).expect("create dashboard logo");
    PngEncoder::new(logo)
        .write_image(
            &mark::rgba(128, mark::IDLE),
            128,
            128,
            ExtendedColorType::Rgba8,
        )
        .expect("encode dashboard logo");

    #[cfg(windows)]
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_windows_icon(&output);
    }
}

#[cfg(windows)]
fn embed_windows_icon(output: &std::path::Path) {
    use image::codecs::ico::{IcoEncoder, IcoFrame};
    use std::{fs, process::Command};

    let frames = [16, 24, 32, 48, 64, 128, 256].map(|size| {
        IcoFrame::as_png(
            &mark::rgba(size, mark::IDLE),
            size,
            size,
            ExtendedColorType::Rgba8,
        )
        .expect("encode Windows icon frame")
    });
    let icon = File::create(output.join("listen-rust.ico")).expect("create Windows icon");
    IcoEncoder::new(icon)
        .encode_images(&frames)
        .expect("write Windows icon");
    fs::write(
        output.join("listen-rust.rc"),
        "1 ICON \"listen-rust.ico\"\n",
    )
    .expect("write Windows icon resource script");

    let sdk = find_msvc_tools::find_windows_sdk(env::consts::ARCH);
    let rc = sdk
        .as_ref()
        .and_then(|sdk| {
            sdk.path()
                .map(|bin| bin.join("rc.exe"))
                .find(|path| path.is_file())
        })
        .unwrap_or_else(|| PathBuf::from("rc.exe"));
    let status = Command::new(rc)
        .current_dir(output)
        .args(["/nologo", "/fo", "listen-rust.res", "listen-rust.rc"])
        .status()
        .expect("run the Windows SDK resource compiler (rc.exe)");
    assert!(
        status.success(),
        "could not compile the Windows icon resource"
    );
    println!(
        "cargo:rustc-link-arg-bin=listen-rs={}",
        output.join("listen-rust.res").display()
    );
}
