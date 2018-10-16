//! Parsing, planning, and executing queries.
//!
//! We use a few central types and traits for running queries:
//!
//! - [`Expr`][expr] represents a syntactically well-formed expression.
//!   This is what `query::ExprParser` constructs.
//!
//! - The [`Plan`][plan] trait is for types that represent a specific plan of
//!   execution. This has a `run` method which actually returns the value
//!   resulting from a particular evaluation. Queries are compiled to a
//!   `Box<Plan>`.
//!
//! - The [`DynEnv`][dye] struct represents the dynamic environment in which
//!   evaluation takes place. At the moment, this just carries the `CoreDump`
//!   around, but as the language becomes more capable, perhaps there will be
//!   more things here.
//!
//! - The [`Value`][value] type represents a run-time value: a number, string, node,
//!   edge, or stream thereof.
//!
//! An `Expr` represents an expression that is well-formed syntactically, but
//! may not correspond to the evaluation strategy we want to use. For example:
//!
//!     nodes { id: 0x12345 }
//!
//! This expression produces a stream of all the nodes in the `CoreDump` just to
//! find the one with the given id. This is extremely inefficient: it should
//! simply try to look up the node with the given id, and produce a stream of
//! zero or one nodes.
//!
//! Evaluating by simply walking the `Expr` directly is straightforward, but it
//! intertwines the code for performing optimizations with the code for actually
//! carrying out the computation, so everything gets a little more difficult to
//! work with.
//!
//! Instead, we perform optimization ('planning') up front, and produce a tree
//! of values that implements the `Plan` trait, which can be run to yield a
//! `Value` or an `Error`.
//!
//! [expr](ast/enum.Expr.html)
//! [plan](run/trait.Plan.html)
//! [value](value/enum.Value.html)
mod ast;
mod breadth_first;
mod env;
mod run;
mod value;
mod walkers;

mod test_utils;
mod test;

mod grammar {
    include!(concat!(env!("OUT_DIR"), "/query/query.rs"));
}

pub use self::grammar::Token;
pub use self::value::{EvalResult, Value};

use dump::CoreDump;
use self::run::label_exprs;

pub type ParseError<'input> = lalrpop_util::ParseError<usize, Token<'input>, &'static str>;

use self::grammar::QueryParser;
use self::run::plan_expr;

use std::fmt;

pub fn compile(query_text: &str) -> Result<Box<Plan>, ParseError> {
    let mut expr = QueryParser::new().parse(&query_text)?;
    label_exprs(&mut expr);
    eprintln!("labeled expr: {:?}", expr);
    env::debug_captures(&expr);
    let plan = plan_expr(&expr);
    eprintln!("plan: {:#?}", plan);
    Ok(plan)
}

/// An error raised during query planning.
#[derive(Clone, Fail, Debug)]
pub enum StaticError {
    #[fail(display = "unbound variable '{}'", name)]
    UnboundVar { name: String },
}

/// A plan of evaluation. We translate each query expression into a tree of
/// `Plan` values, which serve as the code for a sort of indirect-threaded
/// interpreter.
pub trait Plan: fmt::Debug {
    /// Evaluate code for some expression, yielding either a `T` value or an
    /// error. Consult `DynEnv` for random contextual information like the
    /// current `CoreDump`.
    fn run<'a>(&'a self, dye: &'a DynEnv<'a>) -> EvalResult<'a>;
}

/// A plan for evaluating a predicate on a `Value`.
pub trait PredicatePlan: fmt::Debug {
    /// Determine whether this predicate matches `value`. Consult `DynEnv` for
    /// random contextual information like the current `CoreDump`.
    fn test<'a>(&'a self, dye: &'a DynEnv<'a>, &Value<'a>) -> Result<bool, value::Error>;
}

pub struct DynEnv<'a> {
    pub dump: &'a CoreDump<'a>
}
