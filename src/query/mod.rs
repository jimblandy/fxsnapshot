//! Parsing, type-checking, planning, and executing queries.
//!
//! We use a few central types and traits for running queries:
//!
//! - [`Expr`][expr] represents a syntactically well-formed expression.
//!   This is what `query::ExprParser` constructs.
//!
//! - The [`Plan`][plan] trait is for types that represent a specific plan of
//!   execution. This has a `run` method which actually returns the value
//!   resulting from a particular evaluation.
//!
//! - The [`DynEnv`][dye] struct represents the dynamic environment in which
//!   evaluation takes place. At the moment, this just carries the `CoreDump`
//!   around, but as the language becomes more capable, perhaps there will be
//!   more things here.
//!
//! - The [`Value`][value] type represents a run-time value: a number, string, node,
//!   edge, or stream thereof.
//!
//! A `Expr` represents an expression that is syntactically well-formed,
//! but may not be well-typed, and may not correspond to the evaluation strategy
//! we want to use. For example:
//!
//!     10 { id: 20 }
//!
//! is syntactically well-formed, but integers don't have fields, and are not
//! streams to be filtered, so this expression is meaningless. Or consider:
//!
//!     nodes { id: 0x12345 }
//!
//! This expression is well-typed, but producing a stream of all the nodes just
//! to find the one with the given id is extremely inefficient. This should
//! simply check the `CoreDump` for a node with the given id, and produce a
//! stream of zero or one nodes.
//!
//! Evaluating by simply walking the `Expr` directly is straightforward, but it
//! intertwines the code for checking types and performing optimizations with
//! the code for actually carrying out the computation, so everything gets a
//! little more difficult to work with.
//!
//! Instead, we perform type checking and optimization ('planning') up front,
//! and produce a tree of values that implements the `Plan` trait, which can be
//! run to yield a `Value` or an `Error`.
//!
//! [expr](ast/enum.Expr.html)
//! [plan](run/trait.Plan.html)
//! [value](value/enum.Value.html)
mod ast;
mod grammar;
mod run;
mod value;

pub use self::ast::Expr;
pub use self::grammar::ExprParser;
pub use self::run::{plan_expr, Plan, DynEnv};
pub use self::value::Value;
