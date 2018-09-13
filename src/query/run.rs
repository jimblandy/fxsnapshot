//! Types representing query execution strategies.
//!
//! The [`plan_expr`][pe] function takes an `Expr` and produces a `Plan` for
//! evaluating it.
//!
//! [pe]: fn.plan_expr.html

#![allow(unused_variables, dead_code)]

use fallible_iterator::{self, FallibleIterator};

use dump::{CoreDump, Edge, Node};
use super::ast::{Expr, NullaryOp, UnaryOp, StreamBinaryOp, Predicate};
use super::value::{self, EvalResult, Value, Stream, TryUnwrap};

/// A plan of evaluation. We translate each query expression into a tree of
/// `Plan` values, which serve as the code for a sort of indirect-threaded
/// interpreter.
pub trait Plan {
    /// Evaluate code for some expression, yielding either a `T` value or an
    /// error. Consult `DynEnv` for random contextual information like the
    /// current `CoreDump`.
    fn run<'a>(&'a self, dye: &'a DynEnv<'a>) -> EvalResult<'a>;
}

/// A plan for evaluating a predicate on a `Value`.
pub trait PredicatePlan {
    /// Determine whether this predicate matches `value`. Consult `DynEnv` for
    /// random contextual information like the current `CoreDump`.
    fn test<'a>(&'a self, dye: &'a DynEnv<'a>, &Value<'a>) -> Result<bool, value::Error>;
}

pub struct DynEnv<'a> {
    pub dump: &'a CoreDump<'a>
}

/// Given the expression `expr`, return a `Plan` that will evaluate it.
pub fn plan_expr(expr: &Expr) -> Box<Plan> {
    match expr {
        Expr::Number(n) => Box::new(Const(*n)),
        Expr::String(s) => Box::new(Const(s.clone())),
        Expr::StreamLiteral(elts) => {
            Box::new(StreamLiteral(elts.iter().map(plan_expr).collect()))
        }
        Expr::Nullary(op) => plan_nullary(op),
        Expr::Unary(op, expr) => plan_unary(op, expr),
        Expr::Stream(op, stream, predicate) => plan_stream(op, stream, predicate),
    }
}

fn plan_nullary(op: &NullaryOp) -> Box<Plan> {
    match op {
        NullaryOp::Root => Box::new(Root),
        NullaryOp::Nodes => Box::new(Nodes),
    }
}

fn plan_unary(op: &UnaryOp, expr: &Expr) -> Box<Plan> {
    let expr_plan = plan_expr(expr);
    match op {
        UnaryOp::First => Box::new(First(expr_plan)),
        UnaryOp::Edges => Box::new(Edges(expr_plan)),
        UnaryOp::Paths => unimplemented!("UnaryOp::Paths"),
    }
}

fn plan_stream(op: &StreamBinaryOp, stream: &Expr, predicate: &Predicate) -> Box<Plan> {
    let stream = plan_expr(stream);
    let predicate = plan_predicate(predicate);
    match op {
        StreamBinaryOp::Find => unimplemented!("StreamBinaryOp::Find"),
        StreamBinaryOp::Filter => Box::new(Filter { stream, predicate }),
        StreamBinaryOp::Until => unimplemented!("StreamBinaryOp::Until"),
    }
}

fn plan_predicate(predicate: &Predicate) -> Box<PredicatePlan> {
    match predicate {
        Predicate::Expr(expr) => Box::new(EqualPredicate(plan_expr(expr))),
        Predicate::Field(field_name, predicate) =>
            Box::new(FieldPredicate {
                field_name: field_name.clone(),
                predicate: plan_predicate(predicate)
            }),
        Predicate::And(_) => unimplemented!("Predicate::And"),
        Predicate::Or(_) => unimplemented!("Predicate::Or"),
        Predicate::Not(_) => unimplemented!("Predicate::Not"),
    }
}

struct Const<T>(T);

impl<T> Plan for Const<T>
    where T: Clone,
          for<'a> Value<'a>: From<T>
{
    fn run<'a>(&'a self, dye: &'a DynEnv<'a>) -> EvalResult<'a> {
        Ok(Value::from(self.0.clone()))
    }
}

struct StreamLiteral(Vec<Box<Plan>>);

impl Plan for StreamLiteral {
    fn run<'a>(&'a self, dye: &'a DynEnv<'a>) -> EvalResult<'a> {
        let iter = fallible_iterator::convert(self.0.iter().map(move |p| { p.run(dye) }));
        Ok(Value::from(Stream::new(iter)))
    }
}

struct First(Box<Plan>);
impl Plan for First {
    fn run<'a>(&'a self, dye: &'a DynEnv<'a>) -> EvalResult<'a> {
        let value = self.0.run(dye)?;
        let mut stream: Stream<'a> = value.try_unwrap()?;
        match stream.next()? {
            Some(v) => Ok(v),
            None => Err(value::Error::EmptyStream),
        }
    }
}

struct Root;
impl Plan for Root {
    fn run<'a>(&'a self, dye: &'a DynEnv<'a>) -> EvalResult<'a> {
        Ok(Value::from(dye.dump.get_root()))
    }
}

struct Nodes;
impl Plan for Nodes {
    fn run<'a>(&'a self, dye: &'a DynEnv<'a>) -> EvalResult<'a> {
        let iter = fallible_iterator::convert(dye.dump.nodes().map(|n| Ok(n.into())));
        Ok(Value::from(Stream::new(iter)))
    }
}

struct Edges(Box<Plan>);
impl Plan for Edges {
    fn run<'a>(&'a self, dye: &'a DynEnv<'a>) -> EvalResult<'a> {
        let value = self.0.run(dye)?;
        let node: Node = value.try_unwrap()?;
        let iter = node.edges.clone().into_iter().map(|e| Ok(e.into()));
        let iter = fallible_iterator::convert(iter);
        Ok(Stream::new(iter).into())
    }
}

struct Filter {
    stream: Box<Plan>,
    predicate: Box<PredicatePlan>,
}
impl Plan for Filter {
    fn run<'a>(&'a self, dye: &'a DynEnv<'a>) -> EvalResult<'a> {
        let value = self.stream.run(dye)?;
        let stream: Stream = value.try_unwrap()?;
        let iter = stream.filter(move |item| self.predicate.test(&dye, item));
        Ok(Value::from(Stream::new(iter)))
    }
}

struct EqualPredicate(Box<Plan>);
impl PredicatePlan for EqualPredicate {
    fn test<'a>(&'a self, dye: &'a DynEnv<'a>, value: &Value<'a>) -> Result<bool, value::Error> {
        let given = self.0.run(dye)?;
        Ok(*value == given)
    }
}

struct FieldPredicate {
    field_name: String,
    predicate: Box<PredicatePlan>,
}
impl PredicatePlan for FieldPredicate {
    fn test<'a>(&'a self, dye: &'a DynEnv<'a>, value: &Value<'a>) -> Result<bool, value::Error> {
        let field = match value {
            Value::Node(node) => get_node_field(node, &self.field_name)?,
            Value::Edge(edge) => get_edge_field(edge, &self.field_name)?,
            _ => {
                return Err(value::Error::Type {
                    expected: "node or edge",
                    actual: value.type_name()
                });
            }
        };
        field.map_or(Ok(false),
                     |field_value| self.predicate.test(dye, &field_value))
    }
}

fn get_node_field<'v>(node: &'v Node, field: &str)
                      -> Result<Option<Value<'v>>, value::Error>
{
    Ok(match field {
        "id" => Some(node.id.0.into()),
        "size" => node.size.map(Value::from),
        "coarseType" => Some(String::from(node.coarseType).into()),
        "typeName" => node.typeName.map(|t| t.to_string().into()),
        "JSObjectClassName" => node.JSObjectClassName.map(|t| t.to_string().into()),
        "scriptFilename" => node.scriptFilename.map(|t| t.to_string().into()),
        "descriptiveTypeName" => node.descriptiveTypeName.map(|t| t.to_string().into()),
        _ => return Err(value::Error::NoSuchField {
            value_type: "nodes",
            field: field.into()
        }),
    })
}

fn get_edge_field<'v>(edge: &'v Edge, field: &str)
                      -> Result<Option<Value<'v>>, value::Error>
{
    Ok(match field {
        "referent" => edge.referent.map(|id| Value::from(id.0)),
        "name" => edge.name.map(|n| n.to_string().into()),
        _ => return Err(value::Error::NoSuchField {
            value_type: "edges",
            field: field.into()
        }),
    })
}
