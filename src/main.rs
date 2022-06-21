// This is only a problem in the lalrpop output, but because of Clippy
// issue #7290, we have to put this at the top of the crate.
#![allow(clippy::single_component_path_imports)]

#[macro_use]
mod id_vec;

// extern crate uses
use anyhow::{bail, Context, Error};
use memmap::Mmap;

// intra-crate modules
mod dump;
mod query;

// intra-crate uses
use crate::dump::CoreDump;

// std uses
use std::fs::File;
use std::path::Path;

fn run() -> Result<(), Error> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() != 2 {
        bail!("Usage: fxsnapshot FILE QUERY");
    }

    // Compile the query given on the command line.
    let query_text = args[1].to_string_lossy().into_owned();
    let query = query::compile(&query_text)?;

    // Open and index the core dump file.
    let path = Path::new(&args[0]);
    let file =
        File::open(path).context(format!("Failed to open snapshot '{}':", path.display()))?;
    let mmap = unsafe { Mmap::map(&file)? };
    let bytes = &mmap[..];
    let dump = CoreDump::from_bytes(path, bytes)?;

    // Run the query, and print the result to stdout.
    let context = query::Context::from_dump(&dump);
    let activation_base = query::ActivationBase::from_context(&context);
    let activation = query::Activation::for_eval(&activation_base);
    let result = query.run(&activation, &context)?;

    let stdout = std::io::stdout();
    result .top_write(&mut stdout.lock())?;
    println!();

    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{:#}", e);
        std::process::exit(1);
    }
}
