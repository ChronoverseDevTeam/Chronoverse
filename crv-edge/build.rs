fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 使用内置的protoc二进制，避免本机安装依赖
    let protoc = protoc_bin_vendored::protoc_bin_path().expect("Failed to get vendored protoc");
    // Rust 2024 中 set_var 变为 unsafe，此处构建脚本是单线程执行，使用unsafe包裹
    unsafe {
        std::env::set_var("PROTOC", &protoc);
    }

    // 编译 proto 文件，输出到 OUT_DIR
    tonic_prost_build::configure()
        // 可选配置，比如关闭生成 server、client、改变输出路径等
        // .build_server(false)
        // .out_dir("src/generated")  
        .compile_protos(
            &["proto/server.proto"],
            &["proto"],
        )?;
    // 可选：将包名暴露为编译期环境变量便于lib.rs include自定义路径
    // println!("cargo:rustc-env=MY_PROTO_OUT={}", std::env::var("OUT_DIR").unwrap());
    Ok(())
}