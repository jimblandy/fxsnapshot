extern crate lalrpop;
extern crate pb_rs;

use pb_rs::types::{Config, FileDescriptor};
use std::env;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();

    let config = Config {
        in_file: PathBuf::from("src/dump/CoreDump.proto"),
        import_search_path: vec![PathBuf::from("src/dump")],
        out_file: Path::new(&out_dir).join("out"),
        single_module: false,
        no_output: false,
        error_cycle: true,
    };
    FileDescriptor::write_proto(&config)
        .expect("failed to generate CoreDump.rs from CoreDump.proto");
    println!("cargo:rerun-if-changed=src/dump/CoreDump.proto");

    // Since it's impossible to `include!` a file that starts with `//!`
    // comments, as all `pb_rs`-generated modules do, we need to generate our
    // own module file that we can include.
    File::create(Path::new(&out_dir).join("generated.rs"))
        .expect("error creating $OUT_DIR/generated.rs in build.rs")
        .write_all(b"pub mod mozilla;")
        .expect("error writing to generated.rs from build.rs");

    lalrpop::Configuration::new()
        .use_cargo_dir_conventions()
        .process_file("src/query/query.lalrpop")
        .expect("failed to generate parser");
    println!("cargo:rerun-if-changed=src/query/query.lalrpop");
}
