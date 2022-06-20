use pb_rs::{ConfigBuilder, types::FileDescriptor};
use std::env;
use std::path::Path;

fn generate_coredump_parser(manifest_dir: &str, out_dir: &str) {
    let out_dir = Path::new(out_dir).join("dump");
    if !out_dir.is_dir() {
        std::fs::DirBuilder::new().create(&out_dir).unwrap();
    }

    let proto_file = "src/dump/CoreDump.proto";
    println!("cargo:rerun-if-changed={}", proto_file);
    let config_builder = ConfigBuilder::new(
        &[Path::new(manifest_dir).join(proto_file)],
        None,
        Some(&out_dir),
        &[],
    ).unwrap();

    FileDescriptor::run(&config_builder.build()).unwrap();
}

fn generate_query_parser() {
    lalrpop::Configuration::new()
        .use_cargo_dir_conventions()
        .process_file("src/query/query.lalrpop")
        .expect("failed to generate parser");
    println!("cargo:rerun-if-changed=src/query/query.lalrpop");
}

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let out_dir = env::var("OUT_DIR").unwrap();

    dbg!(&manifest_dir);
    dbg!(&out_dir);

    generate_coredump_parser(&manifest_dir, &out_dir);
    generate_query_parser();
}
