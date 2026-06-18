fn main() -> Result<(), Box<dyn std::error::Error>> {
    let protoc_path = std::env::current_dir()?.join("tmp_protoc").join("bin").join("protoc");
    if protoc_path.exists() {
        std::env::set_var("PROTOC", protoc_path);
    }
    tonic_build::configure()
        .protoc_arg("--experimental_allow_proto3_optional")
        .compile(&["proto/kind.proto"], &["proto"])?;
    Ok(())
}
