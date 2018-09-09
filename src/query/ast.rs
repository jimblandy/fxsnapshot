pub enum Expr {
    Number(u64),
    String(String),
    StreamLiteral(Vec<Expr>),
    Nullary(NullaryOp),
    Prefix(PrefixOp, Box<Expr>),
    Stream(StreamBinaryOp, Box<Expr>, Predicate),
}

pub enum NullaryOp {
    Root,
    Nodes,
}

pub enum PrefixOp {
    First,
    Edges,
    Paths,
}

pub enum StreamBinaryOp {
    Find,
    Filter,
    Until,
}

pub enum Predicate {
    Expr(Box<Expr>),
    Field(String, Box<Predicate>),
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
