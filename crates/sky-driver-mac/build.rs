//! `screencapturekit` is a Swift bridge. Swift's back-deployment shims
//! (`libswiftCompatibility56.a` and friends) ship with the active toolchain, not
//! with the linker's default search path, so point the link at the toolchain that
//! `xcrun` selects. Works for both Xcode and the Command Line Tools.

use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let Ok(output) = Command::new("xcrun").args(["--find", "swift"]).output() else { return };
    let swift = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    // <toolchain>/usr/bin/swift -> <toolchain>/usr/lib/swift/macosx
    if let Some(usr) = swift.parent().and_then(|bin| bin.parent()) {
        let lib = usr.join("lib/swift/macosx");
        if lib.exists() {
            println!("cargo:rustc-link-search=native={}", lib.display());
        }
    }
}
