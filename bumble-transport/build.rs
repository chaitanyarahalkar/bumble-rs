fn main() -> Result<(), Box<dyn std::error::Error>> {
    let protoc = protoc_bin_vendored::protoc_bin_path()?;
    std::env::set_var("PROTOC", protoc);
    tonic_build::configure()
        .build_client(true)
        .build_server(true)
        .compile_protos(
            &[
                "proto/android_emulator.proto",
                "proto/netsim_packet_streamer.proto",
            ],
            &["proto"],
        )?;
    println!("cargo:rerun-if-changed=proto/android_emulator.proto");
    println!("cargo:rerun-if-changed=proto/netsim_common.proto");
    println!("cargo:rerun-if-changed=proto/netsim_startup.proto");
    println!("cargo:rerun-if-changed=proto/netsim_packet_streamer.proto");
    Ok(())
}
