#![allow(unused_variables, dead_code)]

use fallible_iterator::{self, FallibleIterator};

use dump::{CoreDump, Node};
use super::ast::{Expr, NullaryOp, UnaryOp, StreamBinaryOp, Predicate};
use super::value::{self, EvalResult, Value, Stream, TryUnwrap};

impl Expr {
    pub fn eval<'a>(&'a self, dump: &'a CoreDump) -> EvalResult<'a> {
        match self {
            Expr::Number(n) => Ok((*n).into()),
            Expr::String(s) => Ok(s.clone().into()),
            Expr::StreamLiteral(elts) => Ok(stream_literal(elts, dump).into()),
            Expr::Nullary(n) => n.eval(dump),
            Expr::Unary(op, e) => op.eval(e, dump),
            Expr::Stream(op, s, p) => op.eval(s, p, dump),
        }
    }
}

fn stream_literal<'a>(elts: &'a Vec<Expr>, dump: &'a CoreDump) -> Stream<'a>
{
    let iter = elts.iter().map(move |e| e.eval(dump));
    let iter = fallible_iterator::convert(iter);
    Stream::new(iter)
}

impl NullaryOp {
    pub fn eval<'a>(&'a self, dump: &'a CoreDump) -> EvalResult<'a> {
        match self {
            NullaryOp::Root => Ok(dump.get_root().into()),
            NullaryOp::Nodes => {
                let iter = dump.nodes().map(|n| Ok(n.into()));
                let iter = fallible_iterator::convert(iter);
                Ok(Stream::new(iter).into())
            }
        }
    }
}

impl UnaryOp {
    pub fn eval<'a>(&'a self, operand: &'a Expr, dump: &'a CoreDump) -> EvalResult<'a> {
        let value = operand.eval(dump)?;
        match self {
            UnaryOp::First => {
                let mut stream: Stream<'a> = value.try_unwrap()?;
                match stream.next()? {
                    Some(v) => Ok(v),
                    None => Err(value::Error::EmptyStream),
                }
            }
            UnaryOp::Edges => {
                let node: Node<'a> = value.try_unwrap()?;
                let iter = node.edges.clone().into_iter()
                    .map(|e| Ok(e.into()));
                let iter = fallible_iterator::convert(iter);
                Ok(Stream::new(iter).into())
            }
            UnaryOp::Paths => unimplemented!("UnaryOp::Paths"),
        }
    }
}

impl StreamBinaryOp {
    pub fn eval<'a>(&'a self, stream_expr: &'a Expr, predicate: &'a Predicate, dump: &'a CoreDump)
        -> EvalResult<'a>
    {
        let stream: Stream = stream_expr.eval(dump)?.try_unwrap()?;

        match self {
            StreamBinaryOp::Find => unimplemented!("StreamBinaryOp::Find"),
            StreamBinaryOp::Filter => {
                let iter = stream.filter(move |item| predicate.eval(&item, dump));
                Ok(Value::Stream(Stream::new(iter)))
            }
            StreamBinaryOp::Until => unimplemented!("StreamBinaryOp::Until"),
        }
    }
}

impl Predicate {
    pub fn eval(&self, operand: &Value, dump: &CoreDump) -> Result<bool, value::Error>
    {
        match self {
            Predicate::Expr(e) => {
                let rhs = e.eval(dump)?;
                Ok(*operand == rhs)
            }
            Predicate::Field(name, predicate) => {
                match operand {
                    Value::Node(node) => {
                        match get_node_field(node, name)? {
                            Some(field) => predicate.eval(&field, dump),
                            None => Ok(false),
                        }
                    }
                    Value::Edge(node) => unimplemented!("edge field predicates"),
                    _ => Err(value::Error::Type {
                        expected: "node or edge",
                        actual: operand.type_name()
                    }),
                }
            }
            Predicate::And(_) => unimplemented!("Predicate::And"),
            Predicate::Or(_) => unimplemented!("Predicate::Or"),
            Predicate::Not(_) => unimplemented!("Predicate::Not"),
        }
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
