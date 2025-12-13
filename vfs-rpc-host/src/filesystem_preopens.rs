// WASI Filesystem Preopens Host Implementation
//
// Implements wasi:filesystem/preopens interface by forwarding to RPC adapter
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
        eprintln!("[VFS-RPC-HOST] get_directories() called");

        // Get RPC directories
        let rpc_dirs = {
            // Lock shared RPC core
            let core = self.lock_rpc_core()?;
            eprintln!("[VFS-RPC-HOST] Locked RPC core");

            // Call RPC adapter's get_directories
            let rpc_store_arc = core.rpc_store.clone();
            let mut rpc_store = super::lock_rpc_store(&rpc_store_arc)?;

            let result = core
                .rpc_instance
                .wasi_filesystem_preopens()
                .call_get_directories(&mut *rpc_store)?;
            eprintln!(
                "[VFS-RPC-HOST] RPC adapter returned {} directories",
                result.len()
            );
            result
        };

        eprintln!(
            "[VFS-RPC-HOST] Mapping {} RPC descriptors to host descriptors",
            rpc_dirs.len()
        );
        // Map RPC descriptors to host descriptors
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
            let mut core = self.lock_rpc_core()?;
            core.descriptor_map.insert(rep_value, rpc_descriptor);

            host_dirs.push((host_descriptor, path));
        }

        Ok(host_dirs)
    }
}
