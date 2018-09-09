use dump::CoreDump;
use super::ast::Expr;
use super::Value;

impl Expr {
    pub fn eval<'dump>(&self, dump: &CoreDump<'dump>) -> Value<'dump> {
        Value::Number(0)
    }
}
