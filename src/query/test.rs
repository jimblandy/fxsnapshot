#![cfg(test)]

use super::QueryParser;
use super::ast::{Expr, Predicate, PredicateOp, Var};

// Quick functions for building expressions.
fn root() -> Box<Expr> {
    Box::new(Expr::Var(Var::Root))
}

fn nodes() -> Box<Expr> {
    Box::new(Expr::Var(Var::Nodes))
}

fn pred_op(stream: Box<Expr>, op: PredicateOp, pred: Box<Predicate>) -> Box<Expr> {
    Box::new(Expr::Predicate(stream, op, *pred))
}

fn filter(stream: Box<Expr>, pred: Box<Predicate>) -> Box<Expr> {
    pred_op(stream, PredicateOp::Filter, pred)
}

fn field(name: &str, pred: Box<Predicate>) -> Box<Predicate> {
    Box::new(Predicate::Field(name.to_owned(), pred))
}

fn number(n: u64) -> Box<Expr> {
    Box::new(Expr::Number(n))
}

fn expr_pred(expr: Box<Expr>) -> Box<Predicate> {
    Box::new(Predicate::Expr(expr))
}

fn and1(pred: Box<Predicate>) -> Box<Predicate> {
    Box::new(Predicate::And(vec![*pred]))
}

#[test]
fn parse_query() {
    assert_eq!(QueryParser::new().parse("root").expect("parse failed"),
               root());
    assert_eq!(QueryParser::new().parse("nodes { id: 0x0123456789abcdef }")
               .expect("parse failed"),
               filter(nodes(), and1(field("id", expr_pred(number(0x0123456789abcdef))))));
}
