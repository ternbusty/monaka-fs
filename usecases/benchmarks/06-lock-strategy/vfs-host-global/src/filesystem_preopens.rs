// WASI Filesystem Preopens Host Implementation for fs-global

use super::{FsDescriptorWrapper, VfsHostState};
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
        const O_RDONLY: u32 = 0;
        const O_DIRECTORY: u32 = 0x10000;

        let fd = self
            .shared_vfs
            .open_path_with_flags("/", O_RDONLY | O_DIRECTORY)
            .map_err(|e| anyhow::anyhow!("Failed to open root directory: {:?}", e))?;

        let wrapper = FsDescriptorWrapper {
            fd,
            path: Some("/".to_string()),
        };

        let wrapper_resource: Resource<FsDescriptorWrapper> = self.table.push(wrapper)?;

        const _: () = {
            use std::mem::{align_of, size_of};
            assert!(
                size_of::<Resource<FsDescriptorWrapper>>()
                    == size_of::<Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>>(
                    )
            );
            assert!(
                align_of::<Resource<FsDescriptorWrapper>>()
                    == align_of::<Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>>(
                    )
            );
        };
        let host_descriptor = unsafe {
            std::mem::transmute::<
                Resource<FsDescriptorWrapper>,
                Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
            >(wrapper_resource)
        };

        Ok(vec![(host_descriptor, "/".to_string())])
    }
}
