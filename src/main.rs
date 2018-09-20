// extern crates
#[macro_use] extern crate failure;
#[macro_use] extern crate failure_derive;
             extern crate fallible_iterator;
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

#[test]
fn parse_query() {
    query::ExprParser::new().parse("root")
        .expect("parse failed");
    query::ExprParser::new().parse("nodes { id: 0x0123456789abcdef }")
        .expect("parse failed");
}

fn run() -> Result<(), Error> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();

    if args.len() != 2 {
        return Err(format_err!("Usage: fxsnapshot FILE QUERY"));
    }

    let query_text = args[1].to_string_lossy().into_owned();
    let query = query::QueryParser::new().parse(&query_text)
        .map_err(|e| format_err!("{}", e))?;
    let plan = query::plan_expr(&query);

    let path = Path::new(&args[0]);
    let file = File::open(path)
        .context(format!("Failed to open snapshot '{}':", path.display()))?;
    let mmap = unsafe { Mmap::map(&file)? };
    let bytes = &mmap[..];

    let dump = CoreDump::new(path, bytes)?;
    let dye = query::DynEnv { dump: &dump };

    let stdout = std::io::stdout();
    plan.run(&dye)?.top_write(&mut stdout.lock())?;
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
