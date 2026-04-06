//! halycon-pack: Pack files into vfs-adapter WASM binaries
//!
//! Usage:
//!    halycon-pack embed --mount /data=./local-dir -o output.wasm input.wasm

use anyhow::{Context, Result, bail};
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
    /// Embed a filesystem snapshot into a WASM binary
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

/// Find HALYCON global addresses by parsing exports and globals sections
fn find_halycon_addresses(module_bytes: &[u8]) -> Result<(u32, u32)> {
    use wasmparser::{Parser, Payload};

    let mut ptr_global_idx: Option<u32> = None;
    let mut len_global_idx: Option<u32> = None;
    let mut ptr_addr: Option<u32> = None;
    let mut len_addr: Option<u32> = None;

    // First pass: find the global indices from exports
    for payload in Parser::new(0).parse_all(module_bytes) {
        if let Payload::ExportSection(reader) = payload? {
            for export in reader {
                let export = export?;
                match export.name {
                    "HALYCON_FS_DATA_PTR" => {
                        if let wasmparser::ExternalKind::Global = export.kind {
                            ptr_global_idx = Some(export.index);
                        }
                    }
                    "HALYCON_FS_DATA_LEN" => {
                        if let wasmparser::ExternalKind::Global = export.kind {
                            len_global_idx = Some(export.index);
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    let ptr_idx = ptr_global_idx.context("HALYCON_FS_DATA_PTR export not found")?;
    let len_idx = len_global_idx.context("HALYCON_FS_DATA_LEN export not found")?;

    // Second pass: find the addresses from globals
    let mut global_count = 0u32;
    for payload in Parser::new(0).parse_all(module_bytes) {
        if let Payload::GlobalSection(reader) = payload? {
            for global in reader {
                let global = global?;
                if global_count == ptr_idx {
                    let mut expr_reader = global.init_expr.get_binary_reader();
                    if let Ok(wasmparser::Operator::I32Const { value }) =
                        expr_reader.read_operator()
                    {
                        ptr_addr = Some(value as u32);
                    }
                }
                if global_count == len_idx {
                    let mut expr_reader = global.init_expr.get_binary_reader();
                    if let Ok(wasmparser::Operator::I32Const { value }) =
                        expr_reader.read_operator()
                    {
                        len_addr = Some(value as u32);
                    }
                }
                global_count += 1;
            }
        }
    }

    let ptr = ptr_addr.context("Could not find HALYCON_FS_DATA_PTR address")?;
    let len = len_addr.context("Could not find HALYCON_FS_DATA_LEN address")?;

    println!("Found HALYCON addresses: PTR=0x{:x}, LEN=0x{:x}", ptr, len);

    Ok((ptr, len))
}

/// Embed snapshot into WASM binary by modifying data section
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

    if !is_component {
        bail!("Only WASM Components are supported. Please provide a vfs-adapter component.");
    }

    let output_wasm = embed_into_component(&wasm_bytes, snapshot_bytes)?;

    // Write output
    std::fs::write(output, &output_wasm)?;

    println!("Wrote {} bytes to {}", output_wasm.len(), output.display());

    Ok(())
}

/// Check if a module contains HALYCON globals by looking at exports
fn has_halycon_globals(module_bytes: &[u8]) -> bool {
    use wasmparser::{Parser, Payload};

    for payload in Parser::new(0).parse_all(module_bytes) {
        if let Ok(Payload::ExportSection(reader)) = payload {
            for export in reader.into_iter().flatten() {
                if export.name == "HALYCON_FS_DATA_PTR" || export.name == "HALYCON_FS_DATA_LEN" {
                    return true;
                }
            }
        }
    }
    false
}

/// Recursively search for HALYCON globals in a component and return the target info
/// Returns: Option<(section_type, start, end)> where section_type is 1 for Module, 4 for Component
fn find_halycon_target(bytes: &[u8]) -> Option<(u8, usize, usize)> {
    use wasmparser::{Parser, Payload};

    for payload in Parser::new(0).parse_all(bytes) {
        match payload {
            Ok(Payload::ModuleSection {
                unchecked_range, ..
            }) => {
                let module_bytes = &bytes[unchecked_range.start..unchecked_range.end];
                if has_halycon_globals(module_bytes) {
                    return Some((1, unchecked_range.start, unchecked_range.end));
                }
            }
            Ok(Payload::ComponentSection {
                unchecked_range, ..
            }) => {
                let component_bytes = &bytes[unchecked_range.start..unchecked_range.end];
                // Recursively check if this nested component contains HALYCON
                if find_halycon_target(component_bytes).is_some() {
                    return Some((4, unchecked_range.start, unchecked_range.end));
                }
            }
            _ => {}
        }
    }
    None
}

/// Find and modify the core module inside a WASM component (supports nested components)
fn embed_into_component(component_bytes: &[u8], snapshot_bytes: &[u8]) -> Result<Vec<u8>> {
    // Find the section containing HALYCON globals (may be nested)
    let (section_type, start, end) = find_halycon_target(component_bytes)
        .context("No module with HALYCON globals found in component")?;

    if section_type == 4 {
        // It's a nested ComponentSection - need to process recursively
        println!("Found nested component at bytes {}..{}", start, end);

        let nested_component = &component_bytes[start..end];

        // Recursively embed into the nested component
        let modified_nested = embed_into_component(nested_component, snapshot_bytes)?;

        // Find section header start for the component section
        let section_header_start = find_section_header_start(component_bytes, start, 4)?;

        let mut result = Vec::with_capacity(component_bytes.len() + snapshot_bytes.len() + 100);

        // Copy everything before the component section
        result.extend_from_slice(&component_bytes[..section_header_start]);

        // Write new component section
        result.push(4); // ComponentSection ID

        // Write the component content length as LEB128
        write_leb128_u32(&mut result, modified_nested.len() as u32);

        // Write the modified nested component
        result.extend_from_slice(&modified_nested);

        // Copy everything after the original component section
        result.extend_from_slice(&component_bytes[end..]);

        Ok(result)
    } else {
        // It's a ModuleSection - process directly (original logic)
        println!("Found core module at bytes {}..{}", start, end);

        let module_bytes = &component_bytes[start..end];

        // Find HALYCON global addresses dynamically
        let (ptr_addr, len_addr) = find_halycon_addresses(module_bytes)?;

        // Modify the core module
        let modified_module = modify_core_module(module_bytes, snapshot_bytes, ptr_addr, len_addr)?;

        // Find section header start
        let section_header_start = find_section_header_start(component_bytes, start, 1)?;

        let mut result = Vec::with_capacity(component_bytes.len() + snapshot_bytes.len() + 100);

        // Copy everything before the module section
        result.extend_from_slice(&component_bytes[..section_header_start]);

        // Write new module section
        result.push(1); // ModuleSection ID

        // Write the module content length as LEB128
        write_leb128_u32(&mut result, modified_module.len() as u32);

        // Write the modified module
        result.extend_from_slice(&modified_module);

        // Copy everything after the original module section
        result.extend_from_slice(&component_bytes[end..]);

        Ok(result)
    }
}

/// Find where the section header starts (before the content range)
fn find_section_header_start(bytes: &[u8], content_start: usize, section_id: u8) -> Result<usize> {
    // The section content is preceded by: section_id (1 byte) + LEB128 size
    // We need to find where this header starts by working backwards
    // Maximum LEB128 for u32 is 5 bytes, so look back up to 6 bytes

    for lookback in 2..=6 {
        if content_start < lookback {
            continue;
        }
        let potential_start = content_start - lookback;

        // Try to parse from this position
        if bytes[potential_start] == section_id {
            // Check if the next bytes form a valid LEB128
            let mut pos = potential_start + 1;

            loop {
                if pos >= content_start {
                    break;
                }
                let byte = bytes[pos];
                pos += 1;

                if byte & 0x80 == 0 {
                    break;
                }
            }

            // If we ended up exactly at content_start, this is likely the header
            if pos == content_start {
                return Ok(potential_start);
            }
        }
    }

    bail!(
        "Could not find section header start for section ID {}",
        section_id
    )
}

/// Write a u32 as LEB128
fn write_leb128_u32(output: &mut Vec<u8>, mut value: u32) {
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        output.push(byte);
        if value == 0 {
            break;
        }
    }
}

/// Modify a core WASM module to add snapshot data
fn modify_core_module(
    module_bytes: &[u8],
    snapshot_bytes: &[u8],
    ptr_addr: u32,
    len_addr: u32,
) -> Result<Vec<u8>> {
    use wasm_encoder::{
        ConstExpr, DataSection, DataSegment, DataSegmentMode, MemorySection, MemoryType, Section,
    };
    use wasmparser::{Parser, Payload};

    // First, find the existing memory info
    let mut has_data_section = false;
    let mut memory_min_pages: u32 = 0;

    for payload in Parser::new(0).parse_all(module_bytes) {
        match payload? {
            Payload::DataSection(_) => {
                has_data_section = true;
            }
            Payload::MemorySection(reader) => {
                for memory in reader {
                    let memory = memory?;
                    memory_min_pages = memory.initial as u32;
                    println!(
                        "Memory: {} pages (max: {:?})",
                        memory_min_pages, memory.maximum
                    );
                }
            }
            _ => {}
        }
    }

    if !has_data_section {
        bail!("No data section found in core module");
    }

    // Calculate current memory size and where to place snapshot
    let page_size: u32 = 65536;
    let current_memory_size = memory_min_pages * page_size;

    // Calculate how much space we need for the snapshot (aligned to 16 bytes)
    let snapshot_size = ((snapshot_bytes.len() as u32) + 15) & !15;

    // We want to place the snapshot at the END of memory to avoid heap corruption
    // The heap grows from data section end upward, so placing snapshot at top is safe
    // Add 1 extra page for the snapshot if it doesn't fit
    let snapshot_space_needed = snapshot_size + 256; // Extra padding

    // Calculate new memory size if needed
    let new_memory_pages = if current_memory_size >= snapshot_space_needed + 0x200000 {
        // Plenty of room, use current pages
        memory_min_pages
    } else {
        // Need more pages - add enough for 2MB headroom + snapshot
        let needed = (0x200000 + snapshot_space_needed + page_size - 1) / page_size;
        std::cmp::max(memory_min_pages, needed)
    };

    let new_memory_size = new_memory_pages * page_size;

    // Place snapshot near the end of memory (but leave some room for stack at very top)
    // Stack typically starts at memory_max and grows down, so leave 64KB for it
    let snapshot_addr = ((new_memory_size - snapshot_size - page_size) & !15) as u32;

    println!(
        "Current memory: {} pages ({} bytes)",
        memory_min_pages, current_memory_size
    );
    println!(
        "New memory: {} pages ({} bytes)",
        new_memory_pages, new_memory_size
    );
    println!(
        "Placing snapshot at: 0x{:x} (near end of memory)",
        snapshot_addr
    );

    // Now rebuild the module with additional data segments and updated memory
    let mut result = Vec::new();
    let mut modified = false;

    let parser = Parser::new(0);

    for payload in parser.parse_all(module_bytes) {
        let payload = payload?;

        match &payload {
            Payload::MemorySection(reader) => {
                // Build a new memory section with potentially increased pages
                let mut memory_section = MemorySection::new();
                for memory in reader.clone() {
                    let memory = memory?;
                    let new_min = std::cmp::max(memory.initial as u32, new_memory_pages) as u64;
                    memory_section.memory(MemoryType {
                        minimum: new_min,
                        maximum: memory.maximum,
                        memory64: memory.memory64,
                        shared: memory.shared,
                        page_size_log2: memory.page_size_log2,
                    });
                }
                memory_section.append_to(&mut result);
            }
            Payload::DataSection(reader) => {
                // Build a new data section with our additional data
                let mut data_section = DataSection::new();

                // Copy existing data segments
                for data in reader.clone() {
                    let data = data?;
                    match data.kind {
                        wasmparser::DataKind::Active {
                            memory_index,
                            offset_expr,
                        } => {
                            let mut expr_reader = offset_expr.get_binary_reader();
                            let offset = if let Ok(wasmparser::Operator::I32Const { value }) =
                                expr_reader.read_operator()
                            {
                                value
                            } else {
                                bail!("Unsupported data segment offset expression");
                            };
                            data_section.segment(DataSegment {
                                mode: DataSegmentMode::Active {
                                    memory_index,
                                    offset: &ConstExpr::i32_const(offset),
                                },
                                data: data.data.iter().copied(),
                            });
                        }
                        wasmparser::DataKind::Passive => {
                            data_section.segment(DataSegment {
                                mode: DataSegmentMode::Passive,
                                data: data.data.iter().copied(),
                            });
                        }
                    }
                }

                // Add new data segment for the snapshot
                data_section.segment(DataSegment {
                    mode: DataSegmentMode::Active {
                        memory_index: 0,
                        offset: &ConstExpr::i32_const(snapshot_addr as i32),
                    },
                    data: snapshot_bytes.iter().copied(),
                });

                // Add data segment to set HALYCON_FS_DATA_PTR
                let ptr_bytes = snapshot_addr.to_le_bytes();
                data_section.segment(DataSegment {
                    mode: DataSegmentMode::Active {
                        memory_index: 0,
                        offset: &ConstExpr::i32_const(ptr_addr as i32),
                    },
                    data: ptr_bytes.iter().copied(),
                });

                // Add data segment to set HALYCON_FS_DATA_LEN
                let len_bytes = (snapshot_bytes.len() as u32).to_le_bytes();
                data_section.segment(DataSegment {
                    mode: DataSegmentMode::Active {
                        memory_index: 0,
                        offset: &ConstExpr::i32_const(len_addr as i32),
                    },
                    data: len_bytes.iter().copied(),
                });

                // Write the new data section
                // Section::append_to() writes ID + (Encode::encode which writes size + content)
                data_section.append_to(&mut result);
                modified = true;
            }
            _ => {
                // For other sections, copy raw bytes
                if let Some((id, range)) = payload.as_section() {
                    result.push(id);
                    write_leb128_u32(&mut result, (range.end - range.start) as u32);
                    result.extend_from_slice(&module_bytes[range]);
                }
            }
        }
    }

    if !modified {
        bail!("Failed to modify data section");
    }

    // Prepend WASM magic and version
    let mut final_result = Vec::with_capacity(result.len() + 8);
    final_result.extend_from_slice(&module_bytes[..8]); // Copy magic + version
    final_result.append(&mut result);

    Ok(final_result)
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
