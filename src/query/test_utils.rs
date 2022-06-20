#![cfg(test)]

use super::ast::{Expr, LambdaId, Predicate, PredicateOp, UseId, Var};

// Quick functions for building expressions.
pub fn root() -> Box<Expr> {
    Box::new(Expr::Var(Var::Root))
}

pub fn nodes() -> Box<Expr> {
    Box::new(Expr::Var(Var::Nodes))
}

pub fn pred_op(stream: Box<Expr>, op: PredicateOp, predicate: Box<Predicate>) -> Box<Expr> {
    Box::new(Expr::PredicateOp { id: LambdaId(0), stream, op, predicate })
}

pub fn filter(stream: Box<Expr>, pred: Box<Predicate>) -> Box<Expr> {
    pred_op(stream, PredicateOp::Filter, pred)
}

pub fn field(name: &str, pred: Box<Predicate>) -> Box<Predicate> {
    Box::new(Predicate::Field(name.to_owned(), pred))
}

pub fn number(n: u64) -> Box<Expr> {
    Box::new(Expr::Number(n))
}

pub fn expr_pred(expr: Box<Expr>) -> Box<Predicate> {
    Box::new(Predicate::Expr(*expr))
}

pub fn and1(pred: Box<Predicate>) -> Box<Predicate> {
    Box::new(Predicate::And(vec![*pred]))
}

pub fn app(arg: Box<Expr>, fun: Box<Expr>) -> Box<Expr> {
    Box::new(Expr::App { arg, fun })
}

pub fn lambda<'a, F: 'a>(id: usize, formals: F, body: Box<Expr>) -> Box<Expr>
where
    F: IntoIterator<Item = &'a &'static str>,
{
    Box::new(Expr::Lambda {
        id: LambdaId(id),
        formals: formals.into_iter().map(|&f| f.to_owned()).collect(),
        body,
    })
}

pub fn var(id: usize, name: &str) -> Box<Expr> {
    Box::new(Expr::Var(Var::Lexical {
        id: UseId(id),
        name: name.to_owned(),
    }))
}
