fn main() {
    prost_build::compile_protos(&["proto/vfs.proto"], &["proto/"]).unwrap();
}
