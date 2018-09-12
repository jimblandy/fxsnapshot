extern crate lalrpop;
extern crate pb_rs;

use std::path::PathBuf;
use pb_rs::types::{Config, FileDescriptor};

fn main() {
    let config = Config {
        in_file: PathBuf::from("src/dump/CoreDump.proto"),
        out_file: PathBuf::from("src/dump/CoreDump.proto.rs"),
        single_module: false,
        import_search_path: vec![PathBuf::from("src/dump")],
        no_output: false,
        error_cycle: true,
    };
    FileDescriptor::write_proto(&config)
        .expect("failed to generate CoreDump.rs from CoreDump.proto");
    println!("cargo:rerun-if-changed=src/dump/CoreDump.proto");

    lalrpop::Configuration::new()
        .process_file("src/query/grammar.lalrpop")
        .expect("failed to generate parser");
    println!("cargo:rerun-if-changed=src/query/grammar.lalrpop");
}
