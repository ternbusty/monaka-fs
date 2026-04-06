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
