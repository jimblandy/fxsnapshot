#![feature(fnbox)] // for std::boxed::FnBox; see #28796

// extern crates
#[macro_use]
extern crate failure;
#[macro_use]
extern crate failure_derive;
extern crate fallible_iterator;
extern crate lalrpop_util;
extern crate memmap;
extern crate quick_protobuf;
extern crate regex;

// extern crate uses
use failure::{Error, ResultExt};
use memmap::Mmap;

// intra-crate modules
mod dump;
mod query;

// intra-crate uses
use dump::CoreDump;

// std uses
use std::fs::File;
use std::path::Path;

fn run() -> Result<(), Error> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() != 2 {
        return Err(format_err!("Usage: fxsnapshot FILE QUERY"));
    }

    // Compile the query given on the command line.
    let query_text = args[1].to_string_lossy().into_owned();
    let query = query::compile(&query_text).map_err(|e| format_err!("{}", e))?;

    // Open and index the core dump file.
    let path = Path::new(&args[0]);
    let file =
        File::open(path).context(format!("Failed to open snapshot '{}':", path.display()))?;
    let mmap = unsafe { Mmap::map(&file)? };
    let bytes = &mmap[..];
    let dump = CoreDump::from_bytes(path, bytes)?;

    // Run the query, and print the result to stdout.
    let context = query::Context::from_dump(&dump);
    let activation = query::Activation::for_eval();
    let stdout = std::io::stdout();
    query.run(&activation, &context)?.top_write(&mut stdout.lock())?;
    println!();

    Ok(())
}

fn main() {
    if let Err(e) = run() {
        for failure in e.iter_chain() {
            eprintln!("{}", failure);
        }
        std::process::exit(1);
    }
}
