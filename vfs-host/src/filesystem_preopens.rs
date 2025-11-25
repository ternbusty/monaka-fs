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
        // Lock shared VFS core
        let mut core = self.shared_vfs.lock().unwrap();

        // Call VFS adapter's get_directories

        let vfs_dirs = core
            .vfs_instance
            .wasi_filesystem_preopens()
            .call_get_directories(&mut *core.vfs_store.lock().unwrap())?;

        // Map VFS descriptors to host descriptors
        let mut host_dirs = Vec::new();
        for (vfs_descriptor, path) in vfs_dirs {
            // Create a host descriptor resource
            // We push () and then unsafely cast to the right type since Resource<T> is just a u32 wrapper
            let temp_resource = self.table.push(())?;
            let rep_value = temp_resource.rep();

            // Create properly typed resource from rep
            // Resource<T> is just a u32 wrapper, so we can reinterpret cast
            let host_descriptor = unsafe {
                std::mem::transmute::<
                    Resource<()>,
                    Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
                >(temp_resource)
            };

            // Map host descriptor to VFS descriptor
            core.descriptor_map.insert(rep_value, vfs_descriptor);

            host_dirs.push((host_descriptor, path));
        }

        Ok(host_dirs)
    }
}
