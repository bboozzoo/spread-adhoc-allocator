// SPDX-FileCopyrightText: 2025 Maciej Borzecki <maciek.borzecki@gmail.com>
//
// SPDX-License-Identifier: MIT

use std::io;

fn build_git_version() -> Result<String, io::Error> {
    use std::process::Command;

    let args = &["describe", "--always"];
    let output = Command::new("git").args(args).output()?;

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn main() {
    if let Ok(vers) = build_git_version() {
        println!("cargo:rustc-env=BUILD_GIT_VERSION={}", vers);
    }
}
