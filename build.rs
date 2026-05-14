use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rustc-check-cfg=cfg(sparkle_updater)");
    println!("cargo:rerun-if-env-changed=SEWRITER_SPARKLE_FRAMEWORK");
    println!("cargo:rerun-if-changed=src/updater_sparkle.mm");

    let Ok(framework_path) = env::var("SEWRITER_SPARKLE_FRAMEWORK") else {
        return;
    };

    let framework_path = PathBuf::from(framework_path);
    let Some(framework_dir) = framework_path.parent() else {
        panic!("SEWRITER_SPARKLE_FRAMEWORK must point to Sparkle.framework");
    };

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR is set by Cargo"));
    let object = out_dir.join("updater_sparkle.o");
    let target = env::var("TARGET").expect("TARGET is set by Cargo");
    let arch = if target.starts_with("x86_64-") {
        "x86_64"
    } else if target.starts_with("aarch64-") {
        "arm64"
    } else {
        panic!("unsupported Sparkle bridge target: {target}");
    };

    let status = Command::new("clang++")
        .args([
            "-x",
            "objective-c++",
            "-std=c++17",
            "-fobjc-arc",
            "-fmodules",
            "-arch",
            arch,
            "-mmacosx-version-min=10.13",
            "-F",
        ])
        .arg(framework_dir)
        .args(["-c", "src/updater_sparkle.mm", "-o"])
        .arg(&object)
        .status()
        .expect("failed to run clang++ for Sparkle bridge");

    if !status.success() {
        panic!("failed to compile Sparkle bridge");
    }

    println!("cargo:rustc-cfg=sparkle_updater");
    println!(
        "cargo:rustc-link-search=framework={}",
        framework_dir.display()
    );
    println!("cargo:rustc-link-lib=framework=Sparkle");
    println!("cargo:rustc-link-lib=framework=Foundation");
    println!(
        "cargo:rustc-link-arg=-Wl,-rpath,{}",
        framework_dir.display()
    );
    println!("cargo:rustc-link-arg={}", object.display());
}
