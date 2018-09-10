#![allow(unused_variables, dead_code)]

use dump::CoreDump;
use super::ast::Expr;
use super::value::{Value, Stream};

impl Expr {
    pub fn eval<'a>(&'a self, dump: &'a CoreDump) -> Value<'a> {
        match self {
            Expr::Number(n) => Value::Number(*n),
            Expr::String(s) => Value::String(s.clone()),
            Expr::StreamLiteral(elts) => Value::Stream(stream_literal(elts, dump)),
            Expr::Nullary(n) => unimplemented!("n.eval(dump)"),
            Expr::Prefix(op, e) => unimplemented!("op.eval(e, dump)"),
            Expr::Stream(op, s, p) => unimplemented!("op.eval(s, p, dump)"),
        }
    }
}

fn stream_literal<'a>(elts: &'a Vec<Expr>, dump: &'a CoreDump) -> Stream<'a>
{
    Stream::new(elts.iter().map(move |e| e.eval(dump)))
}
