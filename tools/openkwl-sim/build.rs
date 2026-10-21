//! Build facts for `GET /system/details`: compiler version and build time.

use std::{
    env,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

fn main() {
    let rustc = env::var("RUSTC").unwrap_or_else(|_| "rustc".into());
    let version = Command::new(rustc).arg("--version").output().ok().and_then(|o| String::from_utf8(o.stdout).ok());
    let version = version.as_deref().and_then(|v| v.split_whitespace().nth(1)).unwrap_or("");
    let built = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    println!("cargo:rustc-env=OPENKWL_RUSTC={version}");
    println!("cargo:rustc-env=OPENKWL_BUILT={built}");
    println!("cargo:rerun-if-changed=build.rs");
}
