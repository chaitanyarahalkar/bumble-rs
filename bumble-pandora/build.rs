fn main() -> Result<(), Box<dyn std::error::Error>> {
    let protoc = protoc_bin_vendored::protoc_bin_path()?;
    let protoc_include = protoc_bin_vendored::include_path()?;
    std::env::set_var("PROTOC", protoc);
    tonic_build::configure()
        .build_client(true)
        .build_server(true)
        // Pandora names RPCs `Connect`, which would collide with tonic's
        // generated `Client::connect(endpoint)` convenience constructor.
        .build_transport(false)
        .compile_protos(
            &[
                "proto/pandora/host.proto",
                "proto/pandora/security.proto",
                "proto/pandora/l2cap.proto",
            ],
            &[
                "proto",
                protoc_include
                    .to_str()
                    .ok_or("invalid protoc include path")?,
            ],
        )?;
    for proto in ["host", "security", "l2cap"] {
        println!("cargo:rerun-if-changed=proto/pandora/{proto}.proto");
    }
    Ok(())
}
