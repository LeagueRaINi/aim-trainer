//! Embeds the application icon into the Windows executable (Explorer, shortcuts, Alt-Tab).

fn main() {
    const ICON: &str = "assets/icons/aim_trainer.ico";
    println!("cargo:rerun-if-changed={ICON}");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon(ICON)
            .compile()
            .expect("compiling the Windows icon resource (needs the Windows SDK's rc.exe)");
    }
}
