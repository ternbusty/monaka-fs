// WASI Filesystem Preopens Host Implementation
//
// Implements wasi:filesystem/preopens interface by forwarding to RPC adapter component
//

use super::VfsRpcHostState;
use wasmtime::component::Resource;

impl wasmtime_wasi::bindings::sync::filesystem::preopens::Host for VfsRpcHostState {
    fn get_directories(
        &mut self,
    ) -> Result<
        Vec<(
            Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
            String,
        )>,
        anyhow::Error,
    > {
        eprintln!("[DEBUG] VfsRpcHostState::get_directories() called");
        // Get RPC adapter directories
        let rpc_dirs = {
            // Lock shared RPC adapter core
            let core = self
                .shared_rpc
                .lock()
                .map_err(|e| anyhow::anyhow!("RPC adapter core lock poisoned: {}", e))?;

            // Call RPC adapter's get_directories
            let rpc_store_arc = core.rpc_store.clone();
            let mut rpc_store = rpc_store_arc
                .lock()
                .map_err(|e| anyhow::anyhow!("RPC store lock poisoned: {}", e))?;

            core.rpc_instance
                .wasi_filesystem_preopens()
                .call_get_directories(&mut *rpc_store)?
        };
        eprintln!(
            "[DEBUG] RPC adapter returned {} directories",
            rpc_dirs.len()
        );

        // Map RPC adapter descriptors to host descriptors
        let mut host_dirs = Vec::new();
        for (rpc_descriptor, path) in rpc_dirs {
            // Create a host descriptor resource
            // We push () and then unsafely cast to the right type since Resource<T> is just a u32 wrapper
            let temp_resource = self.table.push(())?;
            let rep_value = temp_resource.rep();

            // Create properly typed resource from rep
            // Resource<T> is just a u32 wrapper, so we can reinterpret cast
            // SAFETY: This relies on Resource<T> being a transparent wrapper around u32
            // Compile-time checks ensure size and alignment match
            const _: () = {
                use std::mem::{align_of, size_of};
                assert!(
                    size_of::<Resource<()>>()
                        == size_of::<
                            Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
                        >()
                );
                assert!(
                    align_of::<Resource<()>>()
                        == align_of::<
                            Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
                        >()
                );
            };
            let host_descriptor = unsafe {
                std::mem::transmute::<
                    Resource<()>,
                    Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
                >(temp_resource)
            };

            // Re-lock core to insert into descriptor map
            let mut core = self
                .shared_rpc
                .lock()
                .map_err(|e| anyhow::anyhow!("RPC adapter core lock poisoned: {}", e))?;
            core.descriptor_map.insert(rep_value, rpc_descriptor);

            eprintln!("[DEBUG] Mapped preopen: {}", path);
            host_dirs.push((host_descriptor, path));
        }

        eprintln!("[DEBUG] Returning {} host directories", host_dirs.len());
        Ok(host_dirs)
    }
}
