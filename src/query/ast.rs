//! A query expression, syntactically well-formed.

use regex;
use std::boxed::FnBox;
use std::fmt;

#[derive(Clone, Debug)]
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

#[derive(Clone)]
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

/// A type that can walk an expression and accumulate information about it.
///
/// At the moment we're only using this for lambdas, so it includes only the
/// features needed for that. But ideally, this would have all sorts of methods
/// for visiting different kinds of nodes, with default definitions that do
/// nothing. Users could override the methods for the sorts of nodes they care
/// about.
///
/// Also, there's more than one way to walk a tree, so consider dividing this
/// into several traits that walk and propagate values in whatever way makes the
/// users clearest, rather than letting it slouch into something too neutral.
pub trait ExprWalkerMut {
    type Error;

    fn visit_expr(&mut self, expr: &mut Expr) -> Result<(), Self::Error> {
        self.visit_expr_children(expr)
    }

    fn visit_expr_children(&mut self, expr: &mut Expr) -> Result<(), Self::Error> {
        match expr {
            Expr::StreamLiteral(elts) => {
                for elt in elts {
                    self.visit_expr(elt)?;
                }
            }

            Expr::Predicate(expr, _, predicate) => {
                self.visit_expr(expr)?;
                self.visit_predicate(predicate)?;
            }

            Expr::App { arg, fun } => {
                self.visit_expr(arg)?;
                self.visit_expr(fun)?;
            }

            Expr::Lambda { body, .. } => {
                self.visit_expr(body)?;
            }

            Expr::Number(_) => (),
            Expr::String(_) => (),
            Expr::Var(_) => (),
        }
        Ok(())
    }

    fn visit_predicate(&mut self, predicate: &mut Predicate) -> Result<(), Self::Error> {
        self.visit_predicate_children(predicate)
    }

    fn visit_predicate_children(&mut self, predicate: &mut Predicate) -> Result<(), Self::Error> {
        match predicate {
            Predicate::Expr(expr) => self.visit_expr(expr),
            Predicate::Field(_name, subpred) => self.visit_predicate(subpred),
            Predicate::Ends(subpred) | Predicate::Any(subpred) |
            Predicate::All(subpred) | Predicate::Not(subpred) =>
                self.visit_predicate(subpred),
            Predicate::Regex(_) => Ok(()),
            Predicate::And(subpreds) | Predicate::Or(subpreds) => {
                for subpred in subpreds {
                    self.visit_predicate(subpred)?;
                }
                Ok(())
            },
        }
    }
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
