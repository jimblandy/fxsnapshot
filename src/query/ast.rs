//! A query expression, syntactically well-formed.

use regex;
use std::boxed::FnBox;

#[derive(Clone, Debug)]
pub enum Expr {
    Number(u64),
    String(String),
    StreamLiteral(Vec<Box<Expr>>),

    Nullary(NullaryOp),
    Unary(Box<Expr>, UnaryOp),
    Predicate(Box<Expr>, PredicateOp, Predicate),

    Var(String),
    Lambda(String, Box<Expr>),
    App { arg: Box<Expr>, fun: Box<Expr> },
}

#[derive(Clone, Debug)]
pub enum NullaryOp {
    Root,
    Nodes,
}

#[derive(Clone, Debug)]
pub enum UnaryOp {
    First,
    Edges,
    Paths,
}

#[derive(Clone, Debug)]
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
