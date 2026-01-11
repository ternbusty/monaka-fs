// WASI Filesystem Preopens Host Implementation
//
// Implements wasi:filesystem/preopens interface by forwarding to VFS adapter

use super::{VfsDescriptorWrapper, VfsHostState};
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
        log::debug!("[VFS-HOST] get_directories() called");

        // Get VFS directories
        let vfs_dirs = {
            // Lock shared VFS core
            let core = self.lock_vfs_core()?;
            log::debug!("[VFS-HOST] Locked VFS core");

            // Call VFS adapter's get_directories
            let vfs_store_arc = core.vfs_store.clone();
            let mut vfs_store = vfs_store_arc
                .lock()
                .map_err(|e| anyhow::anyhow!("VFS store lock poisoned: {}", e))?;

            let result = core
                .vfs_instance
                .wasi_filesystem_preopens()
                .call_get_directories(&mut *vfs_store)?;
            log::debug!(
                "[VFS-HOST] VFS adapter returned {} directories",
                result.len()
            );
            result
        };

        log::debug!(
            "[VFS-HOST] Mapping {} VFS descriptors to host descriptors",
            vfs_dirs.len()
        );

        // Map VFS descriptors to host descriptors
        let mut host_dirs = Vec::new();
        for (vfs_descriptor, path) in vfs_dirs {
            // Push VfsDescriptorWrapper to ResourceTable (proper typed storage)
            let wrapper = VfsDescriptorWrapper(vfs_descriptor);
            let wrapper_resource: Resource<VfsDescriptorWrapper> = self.table.push(wrapper)?;

            // Transmute Resource<VfsDescriptorWrapper> to Resource<Descriptor>
            // SAFETY: Resource<T> is a transparent u32 wrapper, so transmute is safe
            // Compile-time checks ensure size and alignment match
            const _: () = {
                use std::mem::{align_of, size_of};
                assert!(
                    size_of::<Resource<VfsDescriptorWrapper>>()
                        == size_of::<
                            Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
                        >()
                );
                assert!(
                    align_of::<Resource<VfsDescriptorWrapper>>()
                        == align_of::<
                            Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
                        >()
                );
            };
            let host_descriptor = unsafe {
                std::mem::transmute::<
                    Resource<VfsDescriptorWrapper>,
                    Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
                >(wrapper_resource)
            };

            host_dirs.push((host_descriptor, path));
        }

        Ok(host_dirs)
    }
}
