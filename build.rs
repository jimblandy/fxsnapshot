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
    };

    FileDescriptor::write_proto(&config)
        .expect("failed to generate CoreDump.rs from CoreDump.proto");

    lalrpop::process_root()
        .expect("failed to generate parser");
}
