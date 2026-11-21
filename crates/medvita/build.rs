//! Windows resources (icon and version information) and the main thread stack size.
//!
//! Only the icon and the version go in: GPUI already embeds its application manifest, and a second
//! manifest fails the link with `CVT1100 duplicate resource`.

fn main() {
    println!("cargo:rerun-if-changed=resources/windows/medvita.ico");
    println!("cargo:rerun-if-changed=build.rs");
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_os != "windows" {
        return;
    }
    if target_env == "msvc" {
        // 8 MiB main-thread stack, as Zed does: deep GPUI layouts overflow the 1 MiB default.
        println!("cargo:rustc-link-arg-bins=/STACK:8388608");
    }
    let mut resource = winresource::WindowsResource::new();
    resource.set_icon("resources/windows/medvita.ico");
    resource
        .compile()
        .expect("could not compile the Windows resources");
}
