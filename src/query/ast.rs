//! A query expression, syntactically well-formed.

use regex;
use std::boxed::FnBox;
use std::fmt;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Expr {
    Number(u64),
    String(String),
    StreamLiteral(Vec<Box<Expr>>),

    Predicate(Box<Expr>, PredicateOp, Predicate),

    Var(Var),
    App { arg: Box<Expr>, fun: Box<Expr> },
    Lambda { id: LambdaId, formals: Vec<String>, body: Box<Expr> },
}

#[derive(Clone, Copy, Eq, PartialEq, Hash)]
pub struct LambdaId(pub usize);

impl fmt::Debug for LambdaId {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(fmt, "λ{:?}", self.0)
    }
}

#[derive(Clone, Copy, Eq, PartialEq, Hash)]
pub struct UseId(pub usize);

impl fmt::Debug for UseId {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(fmt, "↑{:?}", self.0)
    }
}

#[derive(Clone, Eq, PartialEq)]
pub enum Var {
    // Special names of built-in operators. For now, these are reserved words,
    // not globals.
    Edges,
    First,
    Nodes,
    Paths,
    Root,

    // Reference to a global or local variable.
    Lexical { id: UseId, name: String },
}

impl fmt::Debug for Var {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        let simple = match self {
            Var::Edges => "edges",
            Var::First => "first",
            Var::Nodes => "nodes",
            Var::Paths => "paths",
            Var::Root => "root",
            Var::Lexical { id, name } => {
                return write!(fmt, "{:?}:{:?})", id, name);
            }
        };
        fmt.write_str(simple)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PredicateOp {
    Find,
    Filter,
    Until,
}

#[derive(Clone, Debug)]
pub enum Predicate {
    Expr(Box<Expr>),
    Field(String, Box<Predicate>),
    Ends(Box<Predicate>),
    Any(Box<Predicate>),
    All(Box<Predicate>),
    Regex(regex::Regex),
    And(Vec<Predicate>),
    Or(Vec<Predicate>),
    Not(Box<Predicate>),
}

impl PartialEq for Predicate {
    fn eq(&self, other: &Predicate) -> bool {
        match (self, other) {
            (Predicate::Expr(lhs), Predicate::Expr(rhs)) => lhs == rhs,
            (Predicate::Field(lhsn, lhsp), Predicate::Field(rhsn, rhsp)) => {
                lhsn == rhsn && lhsp == rhsp
            }
            (Predicate::Ends(lhs), Predicate::Ends(rhs)) => lhs == rhs,
            (Predicate::Any(lhs), Predicate::Any(rhs)) => lhs == rhs,
            (Predicate::All(lhs), Predicate::All(rhs)) => lhs == rhs,
            (Predicate::Regex(lhs), Predicate::Regex(rhs)) => {
                lhs.as_str() == rhs.as_str()
            }
            (Predicate::And(lhs), Predicate::And(rhs)) => lhs == rhs,
            (Predicate::Or(lhs), Predicate::Or(rhs)) => lhs == rhs,
            (Predicate::Not(lhs), Predicate::Not(rhs)) => lhs == rhs,
            _ => false,
        }
    }
}

impl Eq for Predicate { }

pub type Builder = Box<FnBox(Box<Expr>) -> Box<Expr>>;

// Given the text of a string literal, `literal`, return the `String` it
// denotes.
pub fn denoted_string(literal: &str) -> String {
    let mut result = String::with_capacity(literal.len());
    let mut iter = literal.chars();
    while let Some(ch) = iter.next() {
        match ch {
            // String literals never end with a backslash.
            '\\' => result.push(iter.next().unwrap()),
            ch => result.push(ch)
        }
    }
    result
}

pub fn denoted_regex(literal: &str) -> String {
    let mut result = String::with_capacity(literal.len());
    let mut iter = literal.chars();
    while let Some(ch) = iter.next() {
        match ch {
            // Regex literals never end with a backslash.
            '\\' => result.push(iter.next().unwrap()),
            ch => result.push(ch)
        }
    }
    result
}
