fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        // Lets the server hand the store's shared buffer to the response without copying.
        // `SetRequest.value` deliberately stays a `Vec<u8>`: a `Bytes` decoded from a
        // request can share tonic's whole receive buffer, which a cached value would
        // then keep alive.
        .bytes([".memorize.GetResponse.value"])
        .compile_protos(&["proto/memorize.proto"], &["proto"])?;
    Ok(())
}
