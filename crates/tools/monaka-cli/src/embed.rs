use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use walkdir::WalkDir;

use fs_core::InodeId;
use fs_core::snapshot::{
    FileContentSnapshot, FileDataSnapshot, FsSnapshot, InodeSnapshot, MetadataSnapshot,
};

use crate::wasm;

/// Run the embed command: embed files into the CLI's bundled vfs-adapter.
pub fn run(output: &PathBuf, mounts: &[String], s3_sync: bool) -> Result<()> {
    let mounts: Vec<(String, PathBuf)> = mounts
        .iter()
        .map(|m| parse_mount(m))
        .collect::<Result<Vec<_>>>()?;

    let snapshot = build_snapshot(&mounts)?;
    let adapter_bytes = wasm::vfs_adapter(s3_sync);
    embed_snapshot_bytes(adapter_bytes, output, &snapshot)
}

/// Verify that the given bytes are a WASM Component, returning an error
/// otherwise. Both `embed` and `compose --mount` reject bare core modules.
fn ensure_wasm_component(wasm_bytes: &[u8]) -> Result<()> {
    let is_component = wasmparser::Parser::new(0)
        .parse_all(wasm_bytes)
        .find_map(|payload| {
            if let Ok(wasmparser::Payload::Version { encoding, .. }) = payload {
                Some(encoding == wasmparser::Encoding::Component)
            } else {
                None
            }
        })
        .unwrap_or(false);

    if !is_component {
        bail!("Only WASM Components are supported.");
    }
    Ok(())
}

/// Embed files into a WASM binary provided as bytes. Used by both `embed` and `compose --mount`.
pub fn embed_into_bytes(wasm_bytes: &[u8], mounts: &[(String, PathBuf)]) -> Result<Vec<u8>> {
    let snapshot = build_snapshot(mounts)?;
    let snapshot_json = serde_json::to_string(&snapshot)?;
    let snapshot_bytes = snapshot_json.as_bytes();

    ensure_wasm_component(wasm_bytes)?;

    embed_into_component(wasm_bytes, snapshot_bytes)
}

fn embed_snapshot_bytes(wasm_bytes: &[u8], output: &Path, snapshot: &FsSnapshot) -> Result<()> {
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

    ensure_wasm_component(wasm_bytes)?;

    let output_wasm = embed_into_component(wasm_bytes, snapshot_bytes)?;
    std::fs::write(output, &output_wasm)?;
    println!("Wrote {} bytes to {}", output_wasm.len(), output.display());
    Ok(())
}

pub fn parse_mount(mount: &str) -> Result<(String, PathBuf)> {
    let parts: Vec<&str> = mount.splitn(2, '=').collect();
    if parts.len() != 2 {
        bail!(
            "Invalid mount format: '{}'. Expected: /virtual-path=./local-path",
            mount
        );
    }
    let virt_path = parts[0].to_string();
    let local_path = PathBuf::from(parts[1]);
    if !local_path.exists() {
        bail!("Local path does not exist: {}", local_path.display());
    }
    Ok((virt_path, local_path))
}

// --- Everything below is the existing embed logic, unchanged ---

fn build_snapshot(mounts: &[(String, PathBuf)]) -> Result<FsSnapshot> {
    let mut next_inode: InodeId = 1;
    let mut inodes: Vec<InodeSnapshot> = Vec::new();
    let mut dir_entries: BTreeMap<InodeId, BTreeMap<String, InodeId>> = BTreeMap::new();

    dir_entries.insert(0, BTreeMap::new());

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
            return 0;
        }

        let mut current_inode: InodeId = 0;

        for part in &parts[..parts.len() - 1] {
            let entries = dir_entries.entry(current_inode).or_default();

            if let Some(&existing_inode) = entries.get(*part) {
                current_inode = existing_inode;
            } else {
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
                    content: FileContentSnapshot::Dir(BTreeMap::new()),
                });

                current_inode = new_inode;
            }
        }

        current_inode
    }

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
                    continue;
                }

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

fn find_monaka_addresses(module_bytes: &[u8]) -> Result<(u32, u32)> {
    use wasmparser::{Parser, Payload};

    let mut ptr_global_idx: Option<u32> = None;
    let mut len_global_idx: Option<u32> = None;
    let mut ptr_addr: Option<u32> = None;
    let mut len_addr: Option<u32> = None;

    for payload in Parser::new(0).parse_all(module_bytes) {
        if let Payload::ExportSection(reader) = payload? {
            for export in reader {
                let export = export?;
                match export.name {
                    "MONAKA_FS_FS_DATA_PTR" => {
                        if let wasmparser::ExternalKind::Global = export.kind {
                            ptr_global_idx = Some(export.index);
                        }
                    }
                    "MONAKA_FS_FS_DATA_LEN" => {
                        if let wasmparser::ExternalKind::Global = export.kind {
                            len_global_idx = Some(export.index);
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    let ptr_idx = ptr_global_idx.context("MONAKA_FS_FS_DATA_PTR export not found")?;
    let len_idx = len_global_idx.context("MONAKA_FS_FS_DATA_LEN export not found")?;

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

    let ptr = ptr_addr.context("Could not find MONAKA_FS_FS_DATA_PTR address")?;
    let len = len_addr.context("Could not find MONAKA_FS_FS_DATA_LEN address")?;

    Ok((ptr, len))
}

fn has_monaka_globals(module_bytes: &[u8]) -> bool {
    use wasmparser::{Parser, Payload};

    for payload in Parser::new(0).parse_all(module_bytes) {
        if let Ok(Payload::ExportSection(reader)) = payload {
            for export in reader.into_iter().flatten() {
                if export.name == "MONAKA_FS_FS_DATA_PTR" || export.name == "MONAKA_FS_FS_DATA_LEN"
                {
                    return true;
                }
            }
        }
    }
    false
}

fn find_monaka_target(bytes: &[u8]) -> Option<(u8, usize, usize)> {
    use wasmparser::{Parser, Payload};

    for payload in Parser::new(0).parse_all(bytes) {
        match payload {
            Ok(Payload::ModuleSection {
                unchecked_range, ..
            }) => {
                let module_bytes = &bytes[unchecked_range.start..unchecked_range.end];
                if has_monaka_globals(module_bytes) {
                    return Some((1, unchecked_range.start, unchecked_range.end));
                }
            }
            Ok(Payload::ComponentSection {
                unchecked_range, ..
            }) => {
                let component_bytes = &bytes[unchecked_range.start..unchecked_range.end];
                if find_monaka_target(component_bytes).is_some() {
                    return Some((4, unchecked_range.start, unchecked_range.end));
                }
            }
            _ => {}
        }
    }
    None
}

fn embed_into_component(component_bytes: &[u8], snapshot_bytes: &[u8]) -> Result<Vec<u8>> {
    let (section_type, start, end) = find_monaka_target(component_bytes)
        .context("No module with MONAKA_FS globals found in component")?;

    if section_type == 4 {
        let nested_component = &component_bytes[start..end];
        let modified_nested = embed_into_component(nested_component, snapshot_bytes)?;
        let section_header_start = find_section_header_start(component_bytes, start, 4)?;

        let mut result = Vec::with_capacity(component_bytes.len() + snapshot_bytes.len() + 100);
        result.extend_from_slice(&component_bytes[..section_header_start]);
        result.push(4);
        write_leb128_u32(&mut result, modified_nested.len() as u32);
        result.extend_from_slice(&modified_nested);
        result.extend_from_slice(&component_bytes[end..]);
        Ok(result)
    } else {
        let module_bytes = &component_bytes[start..end];
        let (ptr_addr, len_addr) = find_monaka_addresses(module_bytes)?;
        let modified_module = modify_core_module(module_bytes, snapshot_bytes, ptr_addr, len_addr)?;
        let section_header_start = find_section_header_start(component_bytes, start, 1)?;

        let mut result = Vec::with_capacity(component_bytes.len() + snapshot_bytes.len() + 100);
        result.extend_from_slice(&component_bytes[..section_header_start]);
        result.push(1);
        write_leb128_u32(&mut result, modified_module.len() as u32);
        result.extend_from_slice(&modified_module);
        result.extend_from_slice(&component_bytes[end..]);
        Ok(result)
    }
}

fn find_section_header_start(bytes: &[u8], content_start: usize, section_id: u8) -> Result<usize> {
    for lookback in 2..=6 {
        if content_start < lookback {
            continue;
        }
        let potential_start = content_start - lookback;

        if bytes[potential_start] == section_id {
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
                }
            }
            _ => {}
        }
    }

    if !has_data_section {
        bail!("No data section found in core module");
    }

    let page_size: u32 = 65536;
    let current_memory_size = memory_min_pages * page_size;
    let snapshot_size = ((snapshot_bytes.len() as u32) + 15) & !15;
    let snapshot_space_needed = snapshot_size + 256;

    let new_memory_pages = if current_memory_size >= snapshot_space_needed + 0x200000 {
        memory_min_pages
    } else {
        let needed = (0x200000 + snapshot_space_needed + page_size - 1) / page_size;
        std::cmp::max(memory_min_pages, needed)
    };

    let new_memory_size = new_memory_pages * page_size;
    let snapshot_addr = ((new_memory_size - snapshot_size - page_size) & !15) as u32;

    let mut result = Vec::new();
    let mut modified = false;

    for payload in Parser::new(0).parse_all(module_bytes) {
        let payload = payload?;

        match &payload {
            Payload::MemorySection(reader) => {
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
                let mut data_section = DataSection::new();

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

                data_section.segment(DataSegment {
                    mode: DataSegmentMode::Active {
                        memory_index: 0,
                        offset: &ConstExpr::i32_const(snapshot_addr as i32),
                    },
                    data: snapshot_bytes.iter().copied(),
                });

                let ptr_bytes = snapshot_addr.to_le_bytes();
                data_section.segment(DataSegment {
                    mode: DataSegmentMode::Active {
                        memory_index: 0,
                        offset: &ConstExpr::i32_const(ptr_addr as i32),
                    },
                    data: ptr_bytes.iter().copied(),
                });

                let len_bytes = (snapshot_bytes.len() as u32).to_le_bytes();
                data_section.segment(DataSegment {
                    mode: DataSegmentMode::Active {
                        memory_index: 0,
                        offset: &ConstExpr::i32_const(len_addr as i32),
                    },
                    data: len_bytes.iter().copied(),
                });

                data_section.append_to(&mut result);
                modified = true;
            }
            _ => {
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

    let mut final_result = Vec::with_capacity(result.len() + 8);
    final_result.extend_from_slice(&module_bytes[..8]);
    final_result.append(&mut result);

    Ok(final_result)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    // --- parse_mount ---------------------------------------------------------

    #[test]
    fn parse_mount_accepts_valid_format() {
        let dir = tempdir().unwrap();
        let arg = format!("/data={}", dir.path().display());

        let (virt, local) = parse_mount(&arg).expect("valid mount");

        assert_eq!(virt, "/data");
        assert_eq!(local, dir.path());
    }

    #[test]
    fn parse_mount_keeps_only_first_equals() {
        // splitn(2, '=') means everything after the first `=` becomes the
        // local path, including more `=`. Document this behaviour.
        let dir = tempdir().unwrap();
        let nested = dir.path().join("a=b");
        fs::create_dir_all(&nested).unwrap();
        let arg = format!("/v={}", nested.display());

        let (virt, local) = parse_mount(&arg).expect("valid mount");

        assert_eq!(virt, "/v");
        assert_eq!(local, nested);
    }

    #[test]
    fn parse_mount_rejects_missing_equals() {
        let err = parse_mount("/no-equals-here").expect_err("must error");
        assert!(
            err.to_string().contains("Invalid mount format"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn parse_mount_rejects_nonexistent_local_path() {
        let err =
            parse_mount("/v=/this/path/should/not/exist/at/all/xyzzy").expect_err("must error");
        assert!(
            err.to_string().contains("does not exist"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn parse_mount_with_existing_file() {
        // The right-hand side may be a file, not just a directory.
        let dir = tempdir().unwrap();
        let file = dir.path().join("hello.txt");
        fs::write(&file, b"hi").unwrap();
        let arg = format!("/data/hello.txt={}", file.display());

        let (virt, local) = parse_mount(&arg).expect("valid mount");
        assert_eq!(virt, "/data/hello.txt");
        assert_eq!(local, file);
    }

    // --- write_leb128_u32 ----------------------------------------------------
    //
    // We pin the encoding for known boundary values, then round-trip a wider
    // range through wasmparser's decoder so any drift in the encoder is
    // caught immediately.

    #[test]
    fn leb128_known_encodings() {
        let cases: &[(u32, &[u8])] = &[
            (0, &[0x00]),
            (1, &[0x01]),
            (127, &[0x7f]),
            (128, &[0x80, 0x01]),
            (255, &[0xff, 0x01]),
            (16_383, &[0xff, 0x7f]),
            (16_384, &[0x80, 0x80, 0x01]),
            (u32::MAX, &[0xff, 0xff, 0xff, 0xff, 0x0f]),
        ];
        for (value, expected) in cases {
            let mut buf = Vec::new();
            write_leb128_u32(&mut buf, *value);
            assert_eq!(buf.as_slice(), *expected, "encoding for {}", value);
        }
    }

    #[test]
    fn leb128_round_trip_matches_wasmparser() {
        // wasmparser is the decoder we always read these bytes back with,
        // so use its `BinaryReader::read_var_u32` as the oracle.
        let values: &[u32] = &[
            0,
            1,
            7,
            127,
            128,
            255,
            1024,
            16_383,
            16_384,
            65_535,
            65_536,
            1 << 20,
            1 << 28,
            u32::MAX - 1,
            u32::MAX,
        ];
        for &v in values {
            let mut buf = Vec::new();
            write_leb128_u32(&mut buf, v);
            let mut reader = wasmparser::BinaryReader::new(&buf, 0);
            let decoded = reader.read_var_u32().expect("decode");
            assert_eq!(decoded, v, "round trip for {}", v);
        }
    }

    // --- WASM helpers --------------------------------------------------------
    //
    // Hand-roll minimal valid bytes via wasm-encoder so the tests are
    // deterministic and don't depend on whether the bundled adapters have
    // been built.

    fn empty_core_module_bytes() -> Vec<u8> {
        wasm_encoder::Module::new().finish()
    }

    fn empty_component_bytes() -> Vec<u8> {
        wasm_encoder::Component::new().finish()
    }

    #[test]
    fn embed_into_bytes_rejects_core_module() {
        let core = empty_core_module_bytes();
        let err = embed_into_bytes(&core, &[]).expect_err("must error");
        assert!(
            err.to_string().contains("WASM Components"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn embed_into_bytes_rejects_garbage() {
        // Bytes that aren't a WASM file at all should fall through the
        // is_component check and report the same user-facing error.
        let err = embed_into_bytes(&[0xff, 0xff, 0xff, 0xff], &[]).expect_err("must error");
        assert!(
            err.to_string().contains("WASM Components"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn embed_into_bytes_rejects_empty_input() {
        let err = embed_into_bytes(&[], &[]).expect_err("must error");
        assert!(
            err.to_string().contains("WASM Components"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn embed_into_bytes_accepts_component_but_fails_without_monaka_globals() {
        // A valid component without monaka globals should pass the
        // is_component gate and fail later, with a different error.
        let component = empty_component_bytes();
        let err = embed_into_bytes(&component, &[]).expect_err("must error");
        let msg = err.to_string();
        assert!(
            !msg.contains("WASM Components"),
            "is_component check incorrectly fired for a real component: {msg}"
        );
        assert!(
            msg.contains("MONAKA_FS") || msg.contains("module") || msg.contains("globals"),
            "expected a downstream error, got: {msg}"
        );
    }

    #[test]
    fn has_monaka_globals_false_for_empty_module() {
        assert!(!has_monaka_globals(&empty_core_module_bytes()));
    }

    #[test]
    fn has_monaka_globals_false_for_module_with_other_exports() {
        // Build a minimal module that exports a global named something else
        // and confirm we don't pick it up as a monaka marker.
        use wasm_encoder::{
            ConstExpr, ExportKind, ExportSection, GlobalSection, GlobalType, Module, ValType,
        };

        let mut globals = GlobalSection::new();
        globals.global(
            GlobalType {
                val_type: ValType::I32,
                mutable: false,
                shared: false,
            },
            &ConstExpr::i32_const(0),
        );

        let mut exports = ExportSection::new();
        exports.export("not_monaka", ExportKind::Global, 0);

        let mut module = Module::new();
        module.section(&globals);
        module.section(&exports);
        let bytes = module.finish();

        assert!(!has_monaka_globals(&bytes));
    }

    #[test]
    fn has_monaka_globals_true_when_monaka_export_present() {
        // Build a module that exports a global named MONAKA_FS_FS_DATA_PTR
        // so we exercise the positive path of the matcher.
        use wasm_encoder::{
            ConstExpr, ExportKind, ExportSection, GlobalSection, GlobalType, Module, ValType,
        };

        let mut globals = GlobalSection::new();
        globals.global(
            GlobalType {
                val_type: ValType::I32,
                mutable: false,
                shared: false,
            },
            &ConstExpr::i32_const(0),
        );

        let mut exports = ExportSection::new();
        exports.export("MONAKA_FS_FS_DATA_PTR", ExportKind::Global, 0);

        let mut module = Module::new();
        module.section(&globals);
        module.section(&exports);
        let bytes = module.finish();

        assert!(has_monaka_globals(&bytes));
    }

    #[test]
    fn find_monaka_target_returns_none_for_component_without_monaka_module() {
        let component = empty_component_bytes();
        assert!(find_monaka_target(&component).is_none());
    }
}
