fn main() {
    // If HALYCON_SNAPSHOT environment variable is set, embed the snapshot at compile time
    if let Ok(snapshot_path) = std::env::var("HALYCON_SNAPSHOT") {
        let path = std::path::Path::new(&snapshot_path);

        // Convert to absolute path (include_bytes! resolves relative paths from source file location)
        let abs_path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            // Resolve relative paths from workspace root (3 levels up from this package)
            let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
            let workspace_root = std::path::Path::new(&manifest_dir)
                .parent()
                .unwrap() // crates/adapters
                .parent()
                .unwrap() // crates
                .parent()
                .unwrap(); // workspace root
            workspace_root.join(path)
        };

        if !abs_path.exists() {
            panic!(
                "HALYCON_SNAPSHOT path does not exist: {}",
                abs_path.display()
            );
        }

        println!("cargo:rerun-if-env-changed=HALYCON_SNAPSHOT");
        println!("cargo:rerun-if-changed={}", abs_path.display());
        println!("cargo:rustc-cfg=halycon_snapshot");
        println!(
            "cargo:rustc-env=HALYCON_SNAPSHOT_PATH={}",
            abs_path.display()
        );
    }
}
