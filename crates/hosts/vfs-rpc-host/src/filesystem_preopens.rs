// WASI Filesystem Preopens Host Implementation
//
// Implements wasi:filesystem/preopens interface by forwarding to RPC adapter

use super::{RpcDescriptorWrapper, VfsRpcHostState};
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
        log::debug!("[VFS-RPC-HOST] get_directories() called");

        // Get RPC directories
        let rpc_dirs = {
            // Lock shared RPC core
            let core = self.lock_rpc_core()?;
            log::debug!("[VFS-RPC-HOST] Locked RPC core");

            // Call RPC adapter's get_directories
            let rpc_store_arc = core.rpc_store.clone();
            let mut rpc_store = super::lock_rpc_store(&rpc_store_arc)?;

            let result = core
                .rpc_instance
                .wasi_filesystem_preopens()
                .call_get_directories(&mut *rpc_store)?;
            log::debug!(
                "[VFS-RPC-HOST] RPC adapter returned {} directories",
                result.len()
            );
            result
        };

        log::debug!(
            "[VFS-RPC-HOST] Mapping {} RPC descriptors to host descriptors",
            rpc_dirs.len()
        );

        // Map RPC descriptors to host descriptors
        let mut host_dirs = Vec::new();
        for (rpc_descriptor, path) in rpc_dirs {
            // Push RpcDescriptorWrapper to ResourceTable (proper typed storage)
            let wrapper = RpcDescriptorWrapper(rpc_descriptor);
            let wrapper_resource: Resource<RpcDescriptorWrapper> = self.table.push(wrapper)?;

            // Transmute Resource<RpcDescriptorWrapper> to Resource<Descriptor>
            // SAFETY: Resource<T> is a transparent u32 wrapper, so transmute is safe
            // Compile-time checks ensure size and alignment match
            const _: () = {
                use std::mem::{align_of, size_of};
                assert!(
                    size_of::<Resource<RpcDescriptorWrapper>>()
                        == size_of::<
                            Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
                        >()
                );
                assert!(
                    align_of::<Resource<RpcDescriptorWrapper>>()
                        == align_of::<
                            Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
                        >()
                );
            };
            let host_descriptor = unsafe {
                std::mem::transmute::<
                    Resource<RpcDescriptorWrapper>,
                    Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
                >(wrapper_resource)
            };

            host_dirs.push((host_descriptor, path));
        }

        Ok(host_dirs)
    }
}
