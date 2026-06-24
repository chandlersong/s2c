use walkdir::WalkDir;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=proto"); // 当 proto 目录有变化时自动重新编译

    // 收集 proto/ 目录下所有的 .proto 文件
    let proto_files: Vec<String> = WalkDir::new("proto")
        .into_iter()
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
        println!("警告: 未找到任何 .proto 文件！");
        return Ok(());
    }

    println!("找到 {} 个 .proto 文件", proto_files.len());

    // 使用 tonic-prost-build 编译所有 proto 文件
    tonic_prost_build::configure()
        .build_server(true) // 生成服务端代码
        .build_client(true) // 生成客户端代码
        .compile_protos(
            &proto_files,
            &["proto".to_string()], // proto 文件的 include 路径
        )?;

    Ok(())
}
