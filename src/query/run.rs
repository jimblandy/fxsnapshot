//! Types representing query execution strategies.
//!
//! The [`plan_expr`][pe] function takes an `Expr` and produces a `Plan` for
//! evaluating it.
//!
//! [pe]: fn.plan_expr.html

use fallible_iterator::{self, FallibleIterator};
use regex;

use dump::{CoreDump, Edge, Node, NodeId};
use super::ast::{Expr, NullaryOp, UnaryOp, StreamBinaryOp, Predicate};
use super::breadth_first::{BreadthFirst, Step};
use super::value::{self, EvalResult, Value, Stream, TryUnwrap};

use std::iter::once;

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
        UnaryOp::Paths => Box::new(Paths(expr_plan)),
    }
}

fn plan_stream(op: &StreamBinaryOp, stream: &Expr, predicate: &Predicate) -> Box<Plan> {
    //let stream_plan = plan_expr(stream);
    //let predicate_plan = plan_predicate(predicate);
    match op {
        StreamBinaryOp::Find => unimplemented!("StreamBinaryOp::Find"),
        StreamBinaryOp::Filter => plan_filter(stream, predicate),
        StreamBinaryOp::Until => unimplemented!("StreamBinaryOp::Until"),
    }
}

fn plan_filter(stream: &Expr, predicate: &Predicate) -> Box<Plan> {
    // Implement `nodes { id: ... }` using `NodesById`, rather than a linear
    // search over all nodes.
    if let Expr::Nullary(NullaryOp::Nodes) = stream {
        // Does the predicate include a required match for a specific node
        // identifier?
        if let Some((id, remainder)) = find_predicate_required_id(predicate) {
            // Efficiently produce a (zero- or one-element) stream of nodes with
            // the given id.
            let nodes = Box::new(NodesById(plan_expr(id)));
            if let Some(remainder) = remainder {
                // Apply whatever parts of the predicate remain.
                return Box::new(Filter {
                    stream: nodes,
                    predicate: plan_predicate(&remainder)
                });
            } else {
                // The predicate contains only an id filter, so `NodesById` is
                // all we need for the entire filter expression.
                return nodes;
            };
        }
    }

    // We can't use NodesById, so just generate a normal filter expression.
    return Box::new(Filter {
        stream: plan_expr(stream),
        predicate: plan_predicate(predicate)
    });
}

/// If `predicate` only admits `Node`s whose id is equal to a specific
/// expression, then return that expression, together with a new `Predicate`
/// representing the parts of `predicate` other than the `id`.
///
/// Note that if we do have to construct a remainder predicate, it must be
/// constructed afresh, since we can't modify the predicate we were handed.
/// Since we use `Box` and not `Rc` in our parse tree, this could end up copying
/// a lot if the remainder predicate is large.
fn find_predicate_required_id(predicate: &Predicate)
                              -> Option<(&Expr, Option<Predicate>)>
{
    match predicate {
        Predicate::Field(name, id_predicate) if name == "id" => {
            if let Predicate::Expr(id_expr) = &**id_predicate {
                return Some((id_expr, None));
            }
        }

        Predicate::And(predicates) => {
            // Conjunctions should always have at least two elements.
            assert!(predicates.len() >= 2);

            // Search the sub-predicates of this conjunction for one that
            // requires a specific id.
            if let Some((i, id, child_remainder)) = predicates.iter()
                .enumerate()
                .find_map(|(i, p)| {
                    find_predicate_required_id(p)
                        .map(|(id, child_remainder)| (i, id, child_remainder))
                })
            {
                if let Some(child_remainder) = child_remainder {
                    // predicates[i] requires a specific id. We've hoisted
                    // out the id expression, so replace predicates[i] with
                    // the remainder.
                    //
                    // If this clone gets expensive, we should probably
                    // start using `Rc` instead of `Box` in the AST.
                    let mut remainder = predicates.clone();
                    remainder[i] = child_remainder;
                    return Some((id, Some(Predicate::And(remainder))));
                } else {
                    // predicates[i] requires a specific id, with no remainder
                    // predicate. Just drop predicates[i] from the conjunction
                    // altogether.

                    // Avoid creating a single-element conjunction.
                    if predicates.len() == 2 {
                        return Some((id, Some(predicates[1-i].clone())));
                    }

                    let mut remainder = predicates.clone();
                    remainder.remove(i);
                    return Some((id, Some(Predicate::And(remainder))));
                }
            }
        }

        // We could also look into conjunctions, to see if any sub-predicate
        // requires a specific id. The nice code for this uses find_map, which
        // isn't stable yet.
        _ => ()
    }

    None
}

fn plan_predicate(predicate: &Predicate) -> Box<PredicatePlan> {
    match predicate {
        Predicate::Expr(expr) => Box::new(EqualPredicate(plan_expr(expr))),
        Predicate::Field(field_name, predicate) =>
            Box::new(FieldPredicate {
                field_name: field_name.clone(),
                predicate: plan_predicate(predicate)
            }),
        Predicate::Ends(predicate) => Box::new(Ends(plan_predicate(predicate))),
        Predicate::Regex(regex) => Box::new(Regex(regex.clone())),
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
    fn run<'a>(&'a self, _dye: &'a DynEnv<'a>) -> EvalResult<'a> {
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

struct NodesById(Box<Plan>);
impl Plan for NodesById {
    fn run<'a>(&'a self, dye: &'a DynEnv<'a>) -> EvalResult<'a> {
        let value = self.0.run(dye)?;
        let id = NodeId(value.try_unwrap()?);
        let optional_node = dye.dump.get_node(id).map(|on| Ok(Value::from(on)));
        let iter = fallible_iterator::convert(optional_node.into_iter());
        Ok(Value::from(Stream::new(iter)))
    }
}

struct Edges(Box<Plan>);
impl Plan for Edges {
    fn run<'a>(&'a self, dye: &'a DynEnv<'a>) -> EvalResult<'a> {
        let value = self.0.run(dye)?;
        let node: &Node = value.try_unwrap()?;
        let iter = node.edges.iter().map(|e| Ok(Value::from(e)));
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

struct Paths(Box<Plan>);
impl Plan for Paths {
    fn run<'a>(&'a self, dye: &'a DynEnv<'a>) -> EvalResult<'a> {
        let value = self.0.run(dye)?;
        let mut traversal = BreadthFirst::new(dye.dump);
        match value {
            Value::Node(node) => traversal.add_start_node(node.id),
            Value::Stream(mut stream) => {
                while let Some(elt) = stream.next()? {
                    let node: &Node<'a> = elt.try_unwrap()?;
                    traversal.add_start_node(node.id);
                }
            }
            other => return Err(value::Error::Type {
                expected: "node or stream of nodes",
                actual: other.type_name(),
            })
        };

        // The traversal produces a stream of paths, where each path is
        // itself a stream of alternating nodes and edges.
        let paths_iter = traversal
            .filter_map(move |path| {
                if path.len() == 0 {
                    None
                } else {
                    // "Don't be too proud of this technological terror you've constructed."
                    let start = dye.dump.get_node(path[0].origin).unwrap();
                    let iter = once(Value::from(start))
                        .chain(path.into_iter()
                               .flat_map(move |Step { edge, .. }| {
                                   // If this edge is participating in a path, it
                                   // must have a referent...
                                   let referent = dye.dump.get_node(edge.referent.unwrap()).unwrap();
                                   once(Value::from(edge))
                                       .chain(once(Value::from(referent)))
                               }))
                        .map(Ok);
                    Some(Stream::new(fallible_iterator::convert(iter)))
                }
            })
            .map(Value::from)
            .map(Ok);
        Ok(Value::from(Stream::new(fallible_iterator::convert(paths_iter))))
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

struct Ends(Box<PredicatePlan>);
impl PredicatePlan for Ends {
    fn test<'a>(&'a self, dye: &'a DynEnv<'a>, value: &Value<'a>) -> Result<bool, value::Error> {
        let stream: &Stream<'a> = value.try_unwrap_ref()?;
        let last = stream.clone().last()?.ok_or(value::Error::EmptyStream)?;
        self.0.test(dye, &last)
    }
}

struct Regex(regex::Regex);
impl PredicatePlan for Regex {
    fn test<'a>(&'a self, _dye: &'a DynEnv<'a>, value: &Value<'a>) -> Result<bool, value::Error> {
        let string: &String = value.try_unwrap_ref()?;
        Ok(self.0.is_match(&string))
    }
}
