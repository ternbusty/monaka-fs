use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let target = env::var("TARGET").unwrap();

    // Only compile C code for wasm32-wasip2 target
    if !target.starts_with("wasm32-wasip") {
        println!("cargo:warning=Skipping C compilation for non-WASI target: {}", target);
        return;
    }

    println!("cargo:rerun-if-changed=main.c");

    // Find clang in WASI SDK or system
    let clang = env::var("CC").unwrap_or_else(|_| {
        // Try common WASI SDK locations
        let candidates = vec![
            "/opt/wasi-sdk/bin/clang",
            "/usr/local/opt/wasi-sdk/bin/clang",
            "clang",
        ];

        for candidate in &candidates {
            if Command::new(candidate).arg("--version").output().is_ok() {
                return candidate.to_string();
            }
        }

        "clang".to_string()
    });

    // Compile C code to object file
    let obj_file = out_dir.join("main.o");
    let status = Command::new(&clang)
        .args(&[
            "--target=wasm32-wasip2",
            "-c",
            "-o",
        ])
        .arg(&obj_file)
        .arg("main.c")
        .status()
        .expect("Failed to compile C code");

    if !status.success() {
        panic!("C compilation failed");
    }

    // Link the object file
    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=static=main");

    // Create static library from object file
    let lib_file = out_dir.join("libmain.a");
    let ar = env::var("AR").unwrap_or_else(|_| "ar".to_string());
    let status = Command::new(&ar)
        .args(&["rcs"])
        .arg(&lib_file)
        .arg(&obj_file)
        .status()
        .expect("Failed to create static library");

    if !status.success() {
        panic!("Library creation failed");
    }
}
