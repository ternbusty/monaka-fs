use anyhow::{Result, bail};

const VFS_ADAPTER: &[u8] = include_bytes!(env!("VFS_ADAPTER_WASM"));
const VFS_ADAPTER_S3: &[u8] = include_bytes!(env!("VFS_ADAPTER_S3_WASM"));
const RPC_ADAPTER: &[u8] = include_bytes!(env!("RPC_ADAPTER_WASM"));
const VFS_RPC_SERVER: &[u8] = include_bytes!(env!("VFS_RPC_SERVER_WASM"));
const VFS_RPC_SERVER_S3: &[u8] = include_bytes!(env!("VFS_RPC_SERVER_S3_WASM"));

pub fn vfs_adapter(s3_sync: bool) -> &'static [u8] {
    if s3_sync { VFS_ADAPTER_S3 } else { VFS_ADAPTER }
}

pub fn rpc_adapter() -> &'static [u8] {
    RPC_ADAPTER
}

pub fn rpc_server(s3_sync: bool) -> &'static [u8] {
    if s3_sync {
        VFS_RPC_SERVER_S3
    } else {
        VFS_RPC_SERVER
    }
}

/// Get a named binary for the extract command.
pub fn get_binary(name: &str, s3_sync: bool) -> Result<&'static [u8]> {
    match name {
        "adapter" => Ok(vfs_adapter(s3_sync)),
        "rpc-adapter" => {
            if s3_sync {
                bail!("rpc-adapter does not have an S3 sync variant");
            }
            Ok(rpc_adapter())
        }
        "server" => Ok(rpc_server(s3_sync)),
        _ => bail!(
            "Unknown binary: {}. Available: adapter, rpc-adapter, server",
            name
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every bundled binary must start with the WASM magic number `\0asm`.
    /// This catches a misconfigured `build.rs` that points at the wrong file.
    fn assert_wasm_magic(bytes: &[u8], label: &str) {
        assert!(
            bytes.starts_with(b"\0asm"),
            "{} does not start with WASM magic (got {:02x?})",
            label,
            &bytes[..bytes.len().min(8)]
        );
    }

    #[test]
    fn vfs_adapter_returns_distinct_binaries() {
        let plain = vfs_adapter(false);
        let s3 = vfs_adapter(true);
        assert_wasm_magic(plain, "vfs_adapter(false)");
        assert_wasm_magic(s3, "vfs_adapter(true)");
        assert_ne!(
            plain.as_ptr(),
            s3.as_ptr(),
            "s3 and non-s3 should be different bundled bytes",
        );
    }

    #[test]
    fn rpc_server_returns_distinct_binaries() {
        let plain = rpc_server(false);
        let s3 = rpc_server(true);
        assert_wasm_magic(plain, "rpc_server(false)");
        assert_wasm_magic(s3, "rpc_server(true)");
        assert_ne!(plain.as_ptr(), s3.as_ptr());
    }

    #[test]
    fn rpc_adapter_returns_wasm() {
        assert_wasm_magic(rpc_adapter(), "rpc_adapter()");
    }

    #[test]
    fn get_binary_dispatches_known_names() {
        for name in ["adapter", "rpc-adapter", "server"] {
            let bytes = get_binary(name, false).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_wasm_magic(bytes, name);
        }
        for name in ["adapter", "server"] {
            let bytes = get_binary(name, true).unwrap_or_else(|e| panic!("{name} (s3): {e}"));
            assert_wasm_magic(bytes, name);
        }
    }

    #[test]
    fn get_binary_rejects_rpc_adapter_with_s3_sync() {
        let err = get_binary("rpc-adapter", true).expect_err("must error");
        assert!(
            err.to_string().contains("S3 sync variant"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn get_binary_rejects_unknown_name() {
        let err = get_binary("bogus", false).expect_err("must error");
        let msg = err.to_string();
        assert!(msg.contains("Unknown binary"), "unexpected error: {msg}");
        // The error should also list the available names so users can correct
        // their typo without consulting docs.
        for available in ["adapter", "rpc-adapter", "server"] {
            assert!(
                msg.contains(available),
                "error message should mention {available}, got: {msg}"
            );
        }
    }

    #[test]
    fn get_binary_rejects_empty_name() {
        let err = get_binary("", false).expect_err("must error");
        assert!(err.to_string().contains("Unknown binary"));
    }
}
