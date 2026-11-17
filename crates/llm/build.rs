//! Linux: the runner finds llama.cpp's libraries beside itself (they are separate files there, see Cargo.toml).
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") && std::env::var_os("CARGO_FEATURE_VULKAN").is_some() {
        println!("cargo:rustc-link-arg-bins=-Wl,-rpath,$ORIGIN");
    }
}
