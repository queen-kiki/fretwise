//! Embeds the Fretwise icon into the Windows exe so Explorer and the
//! taskbar show it (the window icon in main.rs only covers the running app).

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=../../assets/logo/fretwise.ico");
        winresource::WindowsResource::new()
            .set_icon("../../assets/logo/fretwise.ico")
            .compile()
            .expect("failed to embed Windows icon");
    }
}
