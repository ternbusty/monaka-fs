// WASI Filesystem Preopens Host Implementation
//
// Implements wasi:filesystem/preopens interface by forwarding to VFS adapter
//
// Phase 1: Empty implementation to discover required methods

use super::VfsHostState;
use wasmtime::component::Resource;

impl wasmtime_wasi::bindings::sync::filesystem::preopens::Host for VfsHostState {
    fn get_directories(
        &mut self,
    ) -> Result<
        Vec<(
            Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
            String,
        )>,
        anyhow::Error,
    > {
        // Get VFS directories
        let vfs_dirs = {
            // Lock shared VFS core
            let core = self.lock_vfs_core()?;

            // Call VFS adapter's get_directories
            let vfs_store_arc = core.vfs_store.clone();
            let mut vfs_store = vfs_store_arc
                .lock()
                .map_err(|e| anyhow::anyhow!("VFS store lock poisoned: {}", e))?;

            core.vfs_instance
                .wasi_filesystem_preopens()
                .call_get_directories(&mut *vfs_store)?
        };

        // Map VFS descriptors to host descriptors
        let mut host_dirs = Vec::new();
        for (vfs_descriptor, path) in vfs_dirs {
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
            let mut core = self.lock_vfs_core()?;
            core.descriptor_map.insert(rep_value, vfs_descriptor);

            host_dirs.push((host_descriptor, path));
        }

        Ok(host_dirs)
    }
}
