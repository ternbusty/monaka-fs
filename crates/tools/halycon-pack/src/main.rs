//! halycon-pack: Pack files into vfs-adapter WASM binaries
//!
//! Two usage modes:
//!
//! 1. Generate snapshot file (for build-time embedding):
//!    halycon-pack snapshot --mount /data=./local-dir -o snapshot.json
//!    HALYCON_SNAPSHOT=snapshot.json cargo build -p vfs-adapter --target wasm32-wasip2
//!
//! 2. Embed into WASM binary (adds custom section):
//!    halycon-pack embed --mount /data=./local-dir -o output.wasm input.wasm

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use fs_core::InodeId;
use fs_core::snapshot::{
    FileContentSnapshot, FileDataSnapshot, FsSnapshot, InodeSnapshot, MetadataSnapshot,
};

/// Pack files into halycon vfs-adapter WASM binaries
#[derive(Parser, Debug)]
#[command(name = "halycon-pack")]
#[command(about = "Pack files into vfs-adapter WASM binaries")]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Generate a snapshot JSON file from local directories
    /// This can be used with HALYCON_SNAPSHOT env var during vfs-adapter build
    Snapshot {
        /// Output snapshot file (JSON format)
        #[arg(short, long, required = true)]
        output: PathBuf,

        /// Mount a local directory into the virtual filesystem
        /// Format: /virtual-path=./local-path
        #[arg(short, long = "mount", value_name = "MOUNT", required = true)]
        mounts: Vec<String>,
    },

    /// Embed a snapshot into a WASM binary as a custom section
    /// (for tools that can read custom sections)
    Embed {
        /// Input WASM file (vfs-adapter)
        #[arg(required = true)]
        input: PathBuf,

        /// Output WASM file
        #[arg(short, long, required = true)]
        output: PathBuf,

        /// Mount a local directory into the virtual filesystem
        /// Format: /virtual-path=./local-path
        #[arg(short, long = "mount", value_name = "MOUNT")]
        mounts: Vec<String>,
    },
}

/// Build an FsSnapshot from mounted directories
fn build_snapshot(mounts: &[(String, PathBuf)]) -> Result<FsSnapshot> {
    let mut next_inode: InodeId = 1; // 0 is reserved for root
    let mut inodes: Vec<InodeSnapshot> = Vec::new();
    let mut dir_entries: BTreeMap<InodeId, BTreeMap<String, InodeId>> = BTreeMap::new();

    // Create root directory (inode 0)
    dir_entries.insert(0, BTreeMap::new());

    // Helper to get or create parent directories
    fn ensure_parent_dirs(
        path: &str,
        next_inode: &mut InodeId,
        inodes: &mut Vec<InodeSnapshot>,
        dir_entries: &mut BTreeMap<InodeId, BTreeMap<String, InodeId>>,
    ) -> InodeId {
        let parts: Vec<&str> = path
            .trim_start_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();

        if parts.is_empty() {
            return 0; // Root
        }

        let mut current_inode: InodeId = 0;

        // Navigate/create all parent directories (all but the last part)
        for part in &parts[..parts.len() - 1] {
            let entries = dir_entries.entry(current_inode).or_default();

            if let Some(&existing_inode) = entries.get(*part) {
                current_inode = existing_inode;
            } else {
                // Create new directory
                let new_inode = *next_inode;
                *next_inode += 1;

                entries.insert(part.to_string(), new_inode);
                dir_entries.insert(new_inode, BTreeMap::new());

                inodes.push(InodeSnapshot {
                    id: new_inode,
                    metadata: MetadataSnapshot {
                        size: 0,
                        created: 0,
                        modified: 0,
                        permissions: 0o755,
                        is_dir: true,
                    },
                    content: FileContentSnapshot::Dir(BTreeMap::new()), // Will be updated later
                });

                current_inode = new_inode;
            }
        }

        current_inode
    }

    // Process each mount point
    for (virt_path, local_path) in mounts {
        println!("Mounting {} -> {}", local_path.display(), virt_path);

        for entry in WalkDir::new(local_path)
            .follow_links(true)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let local_file_path = entry.path();
            let relative_path = local_file_path
                .strip_prefix(local_path)
                .context("Failed to get relative path")?;

            // Build virtual path
            let virt_file_path = if relative_path.as_os_str().is_empty() {
                virt_path.clone()
            } else {
                format!(
                    "{}/{}",
                    virt_path.trim_end_matches('/'),
                    relative_path.display()
                )
            };

            let metadata = entry.metadata()?;

            if metadata.is_dir() {
                // Ensure parent directories exist and add this directory
                let parent_inode = ensure_parent_dirs(
                    &virt_file_path,
                    &mut next_inode,
                    &mut inodes,
                    &mut dir_entries,
                );

                let dir_name = Path::new(&virt_file_path)
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();

                if dir_name.is_empty() {
                    continue; // Skip root mount point itself
                }

                // Check if directory already exists
                if dir_entries
                    .get(&parent_inode)
                    .map(|e: &BTreeMap<String, InodeId>| e.contains_key(&dir_name))
                    .unwrap_or(false)
                {
                    continue;
                }

                let new_inode = next_inode;
                next_inode += 1;

                dir_entries
                    .entry(parent_inode)
                    .or_default()
                    .insert(dir_name, new_inode);
                dir_entries.insert(new_inode, BTreeMap::new());

                inodes.push(InodeSnapshot {
                    id: new_inode,
                    metadata: MetadataSnapshot {
                        size: 0,
                        created: 0,
                        modified: 0,
                        permissions: 0o755,
                        is_dir: true,
                    },
                    content: FileContentSnapshot::Dir(BTreeMap::new()),
                });
            } else if metadata.is_file() {
                // Ensure parent directories exist
                let parent_inode = ensure_parent_dirs(
                    &virt_file_path,
                    &mut next_inode,
                    &mut inodes,
                    &mut dir_entries,
                );

                let file_name = Path::new(&virt_file_path)
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .context("Invalid file name")?;

                // Read file content
                let content = std::fs::read(local_file_path)?;
                let size = content.len();

                let new_inode = next_inode;
                next_inode += 1;

                dir_entries
                    .entry(parent_inode)
                    .or_default()
                    .insert(file_name.clone(), new_inode);

                inodes.push(InodeSnapshot {
                    id: new_inode,
                    metadata: MetadataSnapshot {
                        size: size as u64,
                        created: 0,
                        modified: 0,
                        permissions: 0o644,
                        is_dir: false,
                    },
                    content: FileContentSnapshot::File(FileDataSnapshot {
                        size,
                        data: content,
                    }),
                });

                println!("  Added file: {} ({} bytes)", virt_file_path, size);
            }
        }
    }

    // Update directory contents from dir_entries
    // First, add root directory
    inodes.insert(
        0,
        InodeSnapshot {
            id: 0,
            metadata: MetadataSnapshot {
                size: 0,
                created: 0,
                modified: 0,
                permissions: 0o755,
                is_dir: true,
            },
            content: FileContentSnapshot::Dir(
                dir_entries
                    .get(&0)
                    .cloned()
                    .unwrap_or_else(|| BTreeMap::<String, InodeId>::new()),
            ),
        },
    );

    // Update all directory inodes with their entries
    for inode in &mut inodes {
        if let FileContentSnapshot::Dir(_) = &inode.content {
            if let Some(entries) = dir_entries.get(&inode.id) {
                let cloned: BTreeMap<String, InodeId> = entries.clone();
                inode.content = FileContentSnapshot::Dir(cloned);
            }
        }
    }

    Ok(FsSnapshot {
        next_inode,
        root_inode: 0,
        inodes,
    })
}

/// Embed snapshot into WASM binary using a custom section
fn embed_snapshot(input: &Path, output: &Path, snapshot: &FsSnapshot) -> Result<()> {
    // Serialize snapshot to JSON
    let snapshot_json = serde_json::to_string(snapshot)?;
    let snapshot_bytes = snapshot_json.as_bytes();

    println!(
        "Snapshot size: {} bytes ({} files)",
        snapshot_bytes.len(),
        snapshot
            .inodes
            .iter()
            .filter(|i| !i.metadata.is_dir)
            .count()
    );

    // Read input WASM
    let wasm_bytes = std::fs::read(input).context("Failed to read input WASM file")?;

    // Check if it's a component or module
    let is_component = wasmparser::Parser::new(0)
        .parse_all(&wasm_bytes)
        .find_map(|payload| {
            if let Ok(wasmparser::Payload::Version { encoding, .. }) = payload {
                Some(encoding == wasmparser::Encoding::Component)
            } else {
                None
            }
        })
        .unwrap_or(false);

    // For both components and modules, we'll use a simple approach:
    // Parse the WASM, add our custom section, and re-emit
    let output_wasm = if is_component {
        embed_into_component(&wasm_bytes, snapshot_bytes)?
    } else {
        embed_into_module(&wasm_bytes, snapshot_bytes)?
    };

    // Write output
    std::fs::write(output, &output_wasm)?;

    println!("Wrote {} bytes to {}", output_wasm.len(), output.display());

    Ok(())
}

/// Embed snapshot into a WASM component
fn embed_into_component(wasm_bytes: &[u8], snapshot_bytes: &[u8]) -> Result<Vec<u8>> {
    use wasm_encoder::CustomSection;

    // Build the custom section
    let custom = CustomSection {
        name: std::borrow::Cow::Borrowed("halycon-snapshot"),
        data: std::borrow::Cow::Borrowed(snapshot_bytes),
    };

    // Encode the section
    let mut section_bytes = Vec::new();
    wasm_encoder::Encode::encode(&custom, &mut section_bytes);

    // Insert the custom section after the magic number and version (8 bytes)
    // This ensures it appears early in the component
    let mut result = Vec::with_capacity(wasm_bytes.len() + section_bytes.len());
    result.extend_from_slice(&wasm_bytes[..8]); // magic + version
    result.extend_from_slice(&section_bytes);
    result.extend_from_slice(&wasm_bytes[8..]);

    Ok(result)
}

/// Embed snapshot into a core WASM module
fn embed_into_module(wasm_bytes: &[u8], snapshot_bytes: &[u8]) -> Result<Vec<u8>> {
    use wasm_encoder::CustomSection;

    // Same approach as component
    let mut result = Vec::with_capacity(wasm_bytes.len() + snapshot_bytes.len() + 100);

    // Build the custom section
    let custom = CustomSection {
        name: std::borrow::Cow::Borrowed("halycon-snapshot"),
        data: std::borrow::Cow::Borrowed(snapshot_bytes),
    };

    // Encode the section
    let mut section_bytes = Vec::new();
    wasm_encoder::Encode::encode(&custom, &mut section_bytes);

    // Insert after magic + version (8 bytes)
    result.extend_from_slice(&wasm_bytes[..8]);
    result.extend_from_slice(&section_bytes);
    result.extend_from_slice(&wasm_bytes[8..]);

    Ok(result)
}

fn parse_mount(mount: &str) -> Result<(String, PathBuf)> {
    let parts: Vec<&str> = mount.splitn(2, '=').collect();
    if parts.len() != 2 {
        anyhow::bail!(
            "Invalid mount format: '{}'. Expected format: /virtual-path=./local-path",
            mount
        );
    }

    let virt_path = parts[0].to_string();
    let local_path = PathBuf::from(parts[1]);

    if !local_path.exists() {
        anyhow::bail!("Local path does not exist: {}", local_path.display());
    }

    Ok((virt_path, local_path))
}

fn main() -> Result<()> {
    let args = Args::parse();

    match args.command {
        Commands::Snapshot { output, mounts } => {
            if mounts.is_empty() {
                anyhow::bail!("At least one --mount is required");
            }

            // Parse mount points
            let mounts: Vec<(String, PathBuf)> = mounts
                .iter()
                .map(|m| parse_mount(m))
                .collect::<Result<Vec<_>>>()?;

            // Build snapshot
            let snapshot = build_snapshot(&mounts)?;

            // Write snapshot to JSON file
            let json = serde_json::to_string_pretty(&snapshot)?;
            std::fs::write(&output, &json)?;

            println!(
                "Wrote snapshot to {} ({} bytes, {} files)",
                output.display(),
                json.len(),
                snapshot
                    .inodes
                    .iter()
                    .filter(|i| !i.metadata.is_dir)
                    .count()
            );
            println!();
            println!("To build vfs-adapter with this snapshot:");
            println!(
                "  HALYCON_SNAPSHOT={} cargo build -p vfs-adapter --target wasm32-wasip2 --release",
                output.canonicalize().unwrap_or(output.clone()).display()
            );
        }

        Commands::Embed {
            input,
            output,
            mounts,
        } => {
            if mounts.is_empty() {
                anyhow::bail!("At least one --mount is required");
            }

            // Parse mount points
            let mounts: Vec<(String, PathBuf)> = mounts
                .iter()
                .map(|m| parse_mount(m))
                .collect::<Result<Vec<_>>>()?;

            // Build snapshot
            let snapshot = build_snapshot(&mounts)?;

            // Embed into WASM
            embed_snapshot(&input, &output, &snapshot)?;
        }
    }

    println!("Done!");

    Ok(())
}
