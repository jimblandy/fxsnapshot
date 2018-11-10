mod ast;
mod breadth_first;
mod stream;
mod fun;
mod run;
mod value;
mod walkers;

mod test;
mod test_utils;

mod grammar {
    include!(concat!(env!("OUT_DIR"), "/query/query.rs"));
}

pub use self::fun::{Activation, ActivationBase, static_analysis};
pub use self::grammar::Token;
pub use self::value::{EvalResult, Value};

use dump::CoreDump;
use self::grammar::QueryParser;
use self::run::plan_expr;
use std::fmt;

/// A plan of evaluation. We translate each query expression into a tree of
/// `Plan` values, which serve as the code for a sort of indirect-threaded
/// interpreter.
pub trait Plan: fmt::Debug {
    /// Execute the plan `self` in the given context and activation, producing
    /// either a `Value` or an error.
    ///
    /// Use `Context::from_dump` to construct an initial `Context`.
    /// `Activation::for_eval` constructs an `Activation` appropriate for
    /// running plans returned by `compile`.
    fn run<'a, 'd>(&self, &'a Activation<'a, 'd>, &Context<'d>) -> EvalResult<'d>;
}

/// A plan for evaluating a predicate on a `Value`.
pub trait PredicatePlan: fmt::Debug {
    /// Determine whether this predicate, executed in the given environment,
    /// matches `value`.
    fn test<'a, 'd>(&self, &Value<'d>, &Activation<'a, 'd>, &Context<'d>) -> Result<bool, value::Error>;
}

/// An execution context: general parameters for the entire query, like which
/// dump it's operating on.
#[derive(Clone)]
pub struct Context<'a> {
    /// The heap snapshot that operators like `nodes` and `root` should consult.
    pub dump: &'a CoreDump<'a>,
}

impl<'a> Context<'a> {
    pub fn from_dump(dump: &'a CoreDump<'a>) -> Context<'a> {
        Context { dump }
    }
}

pub fn compile(query_text: &str) -> Result<Box<Plan>, StaticError> {
    let mut expr = QueryParser::new().parse(&query_text)?;
    let analysis = static_analysis(&mut expr)?;
    let plan = plan_expr(&expr, &analysis);
    eprintln!("plan: {:#?}", plan);
    Ok(plan)
}

pub type ParseError<'input> = lalrpop_util::ParseError<usize, Token<'input>, &'static str>;

/// An error raised during query planning.
#[derive(Clone, Fail, Debug)]
pub enum StaticError {
    #[fail(display = "error parsing query: {}", _0)]
    Parse(String),

    #[fail(display = "unbound variable '{}'", name)]
    UnboundVar { name: String },
}

impl<'input> From<ParseError<'input>> for StaticError {
    fn from(parse_error: ParseError<'input>) -> StaticError {
        StaticError::Parse(parse_error.to_string())
    }
}
