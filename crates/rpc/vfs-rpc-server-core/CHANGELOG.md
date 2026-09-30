# Changelog

## [0.4.0](https://github.com/ternbusty/monaka-fs/compare/vfs-rpc-server-core-v0.3.1...vfs-rpc-server-core-v0.4.0) (2026-09-30)


### Features

* per-file S3 leases and conditional writes for multi-instance sync ([#113](https://github.com/ternbusty/monaka-fs/issues/113)) ([44c5c90](https://github.com/ternbusty/monaka-fs/commit/44c5c908a767348c01025c7c12fccd4586640882))


### Bug Fixes

* **sync:** treat Closed from flush after a full body write as success ([#132](https://github.com/ternbusty/monaka-fs/issues/132)) ([ce321fb](https://github.com/ternbusty/monaka-fs/commit/ce321fbe2c4f1bc6de1eca05ff229fa166b6f536))
* **sync:** verify lease landed after PUT failure to prevent orphaned leases ([#125](https://github.com/ternbusty/monaka-fs/issues/125)) ([159bb92](https://github.com/ternbusty/monaka-fs/commit/159bb9296926afb056787106bd689bccaaf4f098))

## [0.3.1](https://github.com/ternbusty/monaka-fs/compare/vfs-rpc-server-core-v0.3.0...vfs-rpc-server-core-v0.3.1) (2026-08-19)


### Miscellaneous Chores

* **vfs-rpc-server-core:** Synchronize monaka versions

## [0.3.0](https://github.com/ternbusty/monaka-fs/compare/vfs-rpc-server-core-v0.2.5...vfs-rpc-server-core-v0.3.0) (2026-08-10)


### Miscellaneous Chores

* **vfs-rpc-server-core:** Synchronize monaka versions

## [0.2.5](https://github.com/ternbusty/monaka-fs/compare/vfs-rpc-server-core-v0.2.4...vfs-rpc-server-core-v0.2.5) (2026-05-13)


### Miscellaneous Chores

* **vfs-rpc-server-core:** Synchronize monaka versions
