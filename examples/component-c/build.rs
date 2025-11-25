use std::env;

fn main() {
    let target = env::var("TARGET").unwrap();

    // Only compile C code for wasm32-wasip targets
    if !target.starts_with("wasm32-wasip") {
        println!("cargo:warning=Skipping C compilation for non-WASI target: {}", target);
        return;
    }

    println!("cargo:rerun-if-changed=main.c");

    // Set up WASI SDK compiler if available
    let home = env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let wasi_sdk_paths = vec![
        format!("{}/wasi-sdk", home),
        "/opt/wasi-sdk".to_string(),
        "/usr/local/opt/wasi-sdk".to_string(),
    ];

    for path in &wasi_sdk_paths {
        let clang_path = format!("{}/bin/clang", path);
        if std::path::Path::new(&clang_path).exists() {
            env::set_var("CC", clang_path);
            break;
        }
    }

    // Use cc crate to compile C code
    cc::Build::new()
        .file("main.c")
        .opt_level(2)
        .flag("-fno-exceptions")
        .warnings(false)
        .compile("main");
}
