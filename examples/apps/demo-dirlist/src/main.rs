//! Directory operations demo used by the e2e suite to check that mkdir
//! and rmdir travel between instances through S3 directory markers.
//!
//! Usage:
//!   demo-dirlist mkdir <dir>   create the directory (and parents)
//!   demo-dirlist rmdir <dir>   remove the (empty) directory
//!   demo-dirlist ls <dir>      print one `Entry: <name>` line per entry

use std::env;
use std::fs;
use std::process::exit;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("Usage: demo-dirlist <mkdir|rmdir|ls> <dir>");
        exit(2);
    }
    let (op, dir) = (args[1].as_str(), args[2].as_str());

    let result = match op {
        "mkdir" => fs::create_dir_all(dir).map(|_| println!("Created: {}", dir)),
        "rmdir" => fs::remove_dir(dir).map(|_| println!("Removed: {}", dir)),
        "ls" => fs::read_dir(dir).and_then(|entries| {
            let mut names: Vec<String> = entries
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            for name in names {
                println!("Entry: {}", name);
            }
            Ok(())
        }),
        other => {
            eprintln!("Unknown operation: {}", other);
            exit(2);
        }
    };

    if let Err(e) = result {
        eprintln!("{} {} failed: {} ({:?})", op, dir, e, e.kind());
        exit(1);
    }
}
