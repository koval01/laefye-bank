fn main() {
    let protoc = protoc_bin_vendored::protoc_bin_path().expect("protoc");
    unsafe {
        std::env::set_var("PROTOC", protoc);
    }
    prost_build::Config::new()
        .compile_protos(&["proto/laefye/v1/secure.proto"], &["proto"])
        .expect("protobuf codegen");
}
