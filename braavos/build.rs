use std::fs;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let proto_dir = "src/proto";
    let proto_files: Vec<String> = fs::read_dir(proto_dir)?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            if path.extension().map_or(false, |ext| ext == "proto") {
                Some(path.to_string_lossy().into_owned())
            } else {
                None
            }
        })
        .collect();

    if proto_files.is_empty() {
        println!("cargo:warning=No .proto files found in {}", proto_dir);
        return Ok(());
    }

    prost_build::compile_protos(&proto_files, &[proto_dir])?;
    Ok(())
}