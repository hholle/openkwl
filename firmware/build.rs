//! Puts `memory.x` (application slot behind the stock bootloader) on the linker search path
//! and records build facts for `GET /system/details` (compiler version, build time).

use std::{
    env, fs,
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

fn main() {
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    fs::copy("memory.x", out.join("memory.x")).expect("copy memory.x");
    println!("cargo:rustc-link-search={}", out.display());
    println!("cargo:rerun-if-changed=memory.x");

    let rustc = env::var("RUSTC").unwrap_or_else(|_| "rustc".into());
    let version = Command::new(rustc).arg("--version").output().ok().and_then(|o| String::from_utf8(o.stdout).ok());
    let version = version.as_deref().and_then(|v| v.split_whitespace().nth(1)).unwrap_or("");
    let built = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    println!("cargo:rustc-env=OPENKWL_RUSTC={version}");
    println!("cargo:rustc-env=OPENKWL_BUILT={built}");
    println!("cargo:rerun-if-changed=build.rs");
}
