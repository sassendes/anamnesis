fn git_version() -> String {
    let git = std::process::Command::new("git")
        .arg("describe")
        .args(["--always", "--dirty"])
        .output();
    match git {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).trim().to_string(),
        _ => env!("CARGO_PKG_VERSION").to_string(),
    }
}

fn main() {
    println!("cargo:rustc-env=GIT_VERSION={}", git_version());
    println!("cargo:rerun-if-changed=build.rs");
}
