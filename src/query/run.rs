#![allow(unused_variables, dead_code)]

use dump::CoreDump;
use super::ast::Expr;
use super::value::{Value, Stream};

impl Expr {
    pub fn eval<'expr, 'dump: 'expr>(&'expr self, dump: &'dump CoreDump) -> Value<'expr, 'dump> {
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

fn stream_literal<'expr, 'dump: 'expr>(elts: &'expr Vec<Expr>, dump: &'dump CoreDump)
                                       -> Stream<'expr, 'dump>
{
    Stream::new(elts.iter().map(move |e| e.eval(dump)))
}
