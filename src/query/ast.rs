//! A query expression, syntactically well-formed.
#[derive(Clone, Debug)]
pub enum Expr {
    Number(u64),
    String(String),
    StreamLiteral(Vec<Expr>),

    Nullary(NullaryOp),
    Unary(UnaryOp, Box<Expr>),
    Stream(StreamBinaryOp, Box<Expr>, Predicate),
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
pub enum StreamBinaryOp {
    Find,
    Filter,
    Until,
}

#[derive(Clone, Debug)]
pub enum Predicate {
    Expr(Box<Expr>),
    Field(String, Box<Predicate>),
    Ends(Box<Predicate>),
    And(Vec<Predicate>),
    Or(Vec<Predicate>),
    Not(Box<Predicate>),
}

// Given the text of a string literal, `literal`, return the `String` it
// denotes.
pub fn denoted_string(literal: &str) -> String {
    let mut result = String::with_capacity(literal.len());
    let mut iter = literal.chars();
    while let Some(ch) = iter.next() {
        match ch {
            '\\' => continue,
            ch => result.push(ch)
        }
    }
    result
}
