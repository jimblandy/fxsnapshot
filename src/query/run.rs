//! Types representing query execution strategies.
//!
//! The [`plan_expr`][pe] function takes an `Expr` and produces a `Plan` for
//! evaluating it.
//!
//! [pe]: fn.plan_expr.html

use fallible_iterator::{self, FallibleIterator};
use regex;

use dump::{CoreDump, Edge, Node, NodeId};
use super::ast::{Expr, LambdaId, Predicate, PredicateOp, UseId, Var};
use super::breadth_first::{BreadthFirst, Step};
use super::value::{self, EvalResult, Value, Stream, TryUnwrap};
use super::walkers::ExprWalkerMut;

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

#[derive(Default)]
struct ExprLabeler {
    next_lambda: usize,
    next_use: usize,
}

impl ExprLabeler {
    fn new() -> ExprLabeler {
        ExprLabeler::default()
    }

    fn next_lambda(&mut self) -> LambdaId {
        let next = self.next_lambda;
        self.next_lambda = next + 1;
        LambdaId(next)
    }

    fn next_use(&mut self) -> UseId {
        let next = self.next_use;
        self.next_use = next + 1;
        UseId(next)
    }
}

impl<'e> ExprWalkerMut<'e> for ExprLabeler {
    type Error = ();
    fn visit_expr(&mut self, expr: &'e mut Expr) -> Result<(), ()> {
        match expr {
            Expr::Lambda { id, .. } => {
                *id = self.next_lambda();
            }
            Expr::Var(Var::Lexical { id, .. }) => {
                *id = self.next_use();
            }
            _ => ()
        }
        self.visit_expr_children(expr)
    }
}

pub fn label_exprs(expr: &mut Expr) {
    ExprLabeler::new().visit_expr(expr).unwrap();
}

/// Given the expression `expr`, return a `Plan` that will evaluate it.
pub fn plan_expr(expr: &Expr) -> Box<Plan> {
    match expr {
        Expr::Number(n) => Box::new(Const(*n)),
        Expr::String(s) => Box::new(Const(s.clone())),
        Expr::StreamLiteral(elts) => {
            Box::new(StreamLiteral(elts.iter().map(|b| plan_expr(b)).collect()))
        }
        Expr::Predicate(stream, op, predicate) => plan_stream(op, stream, predicate),

        Expr::Var(var) => plan_var(var),
        Expr::Lambda { .. } => unimplemented!("Expr::Lambda"),
        Expr::App { arg, fun } => plan_app(arg, fun),
    }
}

fn plan_var(var: &Var) -> Box<Plan> {
    match var {
        Var::Root => Box::new(Root),
        Var::Nodes => Box::new(Nodes),
        _ => unimplemented!("plan_var"),
    }
}

fn plan_app(arg: &Expr, fun: &Expr) -> Box<Plan> {
    let arg_plan = plan_expr(arg);

    // Handle direct applications of certain built-in functions.
    match fun {
        Expr::Var(Var::Edges) => Box::new(Edges(arg_plan)),
        Expr::Var(Var::First) => Box::new(First(arg_plan)),
        Expr::Var(Var::Paths) => Box::new(Paths(arg_plan)),
        _ => unimplemented!("plan_app"),
    }
}

fn plan_stream(op: &PredicateOp, stream: &Expr, predicate: &Predicate) -> Box<Plan> {
    //let stream_plan = plan_expr(stream);
    //let predicate_plan = plan_predicate(predicate);
    match op {
        PredicateOp::Find => unimplemented!("PredicateOp::Find"),
        PredicateOp::Filter => plan_filter(stream, predicate),
        PredicateOp::Until => unimplemented!("PredicateOp::Until"),
    }
}

fn plan_filter(stream: &Expr, predicate: &Predicate) -> Box<Plan> {
    let stream_plan: Box<Plan>;
    let predicate_plan;

    // Can we implement `nodes { id: ... }` using `NodesById`, rather than a
    // linear search over all nodes?
    match stream {
        Expr::Var(Var::Nodes) => {
            if let Some((id, remainder)) = find_predicate_required_id(predicate) {
                stream_plan = Box::new(NodesById(plan_expr(id)));
                predicate_plan = plan_junction::<And>(&remainder);
            } else {
                stream_plan = Box::new(Nodes);
                predicate_plan = plan_predicate(predicate);
            }
        },
        stream => {
            stream_plan = plan_expr(stream);
            predicate_plan = plan_predicate(predicate);
        }
    }

    match predicate_plan {
        // If the predicate is always true, then the stream is unfiltered.
        PlanOrTrivial::Trivial(true) => stream_plan,

        // If the predicate will never match, then the result is always an empty
        // stream.
        PlanOrTrivial::Trivial(false) => Box::new(StreamLiteral(vec![])),

        // If the predicate is interesting, then filter the result from the
        // stream.
        PlanOrTrivial::Plan(plan) => Box::new(Filter { stream: stream_plan, filter: plan }),
    }
}

/// When we plan a predicate, sometimes we discover that the predicate is always
/// true or always false, and we shouldn't produce an execution plan for it at
/// all. Values of this type are the results of such an effort: either a plan
/// for executing a predicate, or the answer we know it will always return.
enum PlanOrTrivial {
    Plan(Box<PredicatePlan>),
    Trivial(bool)
}

impl PlanOrTrivial {
    fn map_plan<F>(self, f: F) -> PlanOrTrivial
        where F: FnOnce(Box<PredicatePlan>) -> Box<PredicatePlan>
    {
        match self {
            PlanOrTrivial::Plan(plan) => PlanOrTrivial::Plan(f(plan)),
            trivial @ PlanOrTrivial::Trivial(_) => trivial,
        }
    }
}

fn plan_predicate(predicate: &Predicate) -> PlanOrTrivial {
    use self::PlanOrTrivial::*;
    match predicate {
        Predicate::Expr(expr) => Plan(Box::new(EqualPredicate(plan_expr(expr)))),
        Predicate::Field(field_name, sub) => {
            plan_predicate(sub)
                .map_plan(|predicate| Box::new(FieldPredicate {
                    field_name: field_name.clone(),
                    predicate
                }))
        }
        Predicate::Ends(sub) => {
            plan_predicate(sub)
                .map_plan(|subplan| Box::new(Ends(subplan)))
        }
        Predicate::Any(sub) => match plan_predicate(sub) {
            PlanOrTrivial::Plan(p) => Plan(Box::new(Any(p))),
            PlanOrTrivial::Trivial(false) => Trivial(false),
            PlanOrTrivial::Trivial(true) => Plan(Box::new(NonEmpty)),
        }
        Predicate::All(sub) => match plan_predicate(sub) {
            PlanOrTrivial::Plan(p) => Plan(Box::new(All(p))),
            PlanOrTrivial::Trivial(true) => Trivial(true),
            PlanOrTrivial::Trivial(false) => Plan(Box::new(Empty)),
        }
        Predicate::Regex(regex) => Plan(Box::new(Regex(regex.clone()))),
        Predicate::And(predicates) => plan_junction::<And>(predicates),
        Predicate::Or(predicates) => plan_junction::<Or>(predicates),
        Predicate::Not(predicate) => match plan_predicate(predicate) {
            Trivial(k) => Trivial(!k),
            Plan(p) => Plan(Box::new(Not(p))),
        }
    }
}

/// If `predicate` only admits `Node`s whose id is equal to a specific
/// expression, then return that expression, together with a vector of
/// `Predicates` that must also match, representing the parts of `predicate`
/// other than the `id`. These are the subterms of an implicit conjunction. The
/// vector may be empty.
///
/// Note that if we do have to construct a remainder predicate, it must be
/// constructed afresh, since we can't modify the predicate we were handed.
/// Since we use `Box` and not `Rc` in our parse tree, this could end up copying
/// a lot if the remainder predicate is large.
fn find_predicate_required_id(predicate: &Predicate)
                              -> Option<(&Expr, Vec<Predicate>)>
{
    match predicate {
        Predicate::Field(name, id_predicate) if name == "id" => {
            if let Predicate::Expr(id_expr) = &**id_predicate {
                return Some((id_expr, vec![]));
            }
        }

        Predicate::And(predicates) => {
            // Search the sub-predicates of this conjunction for one that
            // requires a specific id.
            if let Some((i, id, child_remainder)) = predicates.iter()
                .enumerate()
                .find_map(|(i, p)| {
                    find_predicate_required_id(p)
                        .map(|(id, child_remainder)| (i, id, child_remainder))
                })
            {
                // predicates[i] requires a specific id. We've hoisted out the
                // id expression, so replace predicates[i] with child_remainder.
                return Some((id, splice(predicates, i, child_remainder)));
            }
        }

        // We could also look into conjunctions, to see if any sub-predicate
        // requires a specific id. The nice code for this uses find_map, which
        // isn't stable yet.
        _ => ()
    }

    None
}

/// Return a vector equal to `slice`, but with `slice[i]` replaced with the
/// elements of `v`.
///
/// This is like `Vec::splice`, except that it clones---but only when necessary.
fn splice<T: Clone>(slice: &[T], i: usize, v: Vec<T>) -> Vec<T> {
    assert!(i < slice.len());
    if slice.len() == 1 {
        v
    } else {
        let mut vec = slice.to_owned();
        vec.splice(i..=i, v.into_iter());
        vec
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
    filter: Box<PredicatePlan>,
}
impl Plan for Filter {
    fn run<'a>(&'a self, dye: &'a DynEnv<'a>) -> EvalResult<'a> {
        let value = self.stream.run(dye)?;
        let stream: Stream = value.try_unwrap()?;
        let iter = stream.filter(move |item| self.filter.test(&dye, item));
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

struct And(Vec<Box<PredicatePlan>>);
impl PredicatePlan for And {
    fn test<'a>(&'a self, dye: &'a DynEnv<'a>, value: &Value<'a>) -> Result<bool, value::Error> {
        fallible_iterator::convert(self.0.iter().map(Ok))
            .all(|plan| plan.test(dye, value))
    }
}

struct Or(Vec<Box<PredicatePlan>>);
impl PredicatePlan for Or {
    fn test<'a>(&'a self, dye: &'a DynEnv<'a>, value: &Value<'a>) -> Result<bool, value::Error> {
        fallible_iterator::convert(self.0.iter().map(Ok))
            .any(|plan| plan.test(dye, value))
    }
}

struct Not(Box<PredicatePlan>);
impl PredicatePlan for Not {
    fn test<'a>(&'a self, dye: &'a DynEnv<'a>, value: &Value<'a>) -> Result<bool, value::Error> {
        Ok(!self.0.test(dye, value)?)
    }
}

/// A predicate plan representing conjunction or disjunction.
trait BlahJunction {
    /// The consonant for `And` is `true`; for `Or`, it is false. The dissonant
    /// is the opposite.
    ///
    /// When a subterm is trivial: If its answer is the consonant, drop that
    /// subterm from the overall expression; if all subterms are dropped this
    /// way, the entire predicate is trivially consonant. If its answer is the
    /// dissonant, the entire predicate is trivially the dissonant.
    const CONSONANT: bool;

    /// Construct an instance from a vector of subterm plans, guaranteed to be
    /// non-empty.
    fn construct(subplans: Vec<Box<PredicatePlan>>) -> Box<PredicatePlan>;
}

impl BlahJunction for And {
    const CONSONANT: bool = true;
    fn construct(subplans: Vec<Box<PredicatePlan>>) -> Box<PredicatePlan> {
        Box::new(And(subplans))
    }
}

impl BlahJunction for Or {
    const CONSONANT: bool = false;
    fn construct(subplans: Vec<Box<PredicatePlan>>) -> Box<PredicatePlan> {
        Box::new(Or(subplans))
    }
}

/// Plan a `BlahJunction` predicate with the given `subterms`.
fn plan_junction<J: BlahJunction>(subterms: &[Predicate]) -> PlanOrTrivial {
    let mut plans = Vec::new();
    for pot in subterms.iter().map(plan_predicate) {
        match pot {
            PlanOrTrivial::Plan(plan) => {
                plans.push(plan);
            }
            PlanOrTrivial::Trivial(k) => {
                if k == !J::CONSONANT {
                    return PlanOrTrivial::Trivial(!J::CONSONANT);
                }
            }
        }
    }

    if plans.is_empty() {
        PlanOrTrivial::Trivial(J::CONSONANT)
    } else {
        PlanOrTrivial::Plan(J::construct(plans))
    }
}

struct Any(Box<PredicatePlan>);
impl PredicatePlan for Any {
    fn test<'a>(&'a self, dye: &'a DynEnv<'a>, value: &Value<'a>) -> Result<bool, value::Error> {
        let stream: &Stream = value.try_unwrap_ref()?;
        Ok(stream.clone().any(|element| self.0.test(dye, &element))?)
    }
}

struct All(Box<PredicatePlan>);
impl PredicatePlan for All {
    fn test<'a>(&'a self, dye: &'a DynEnv<'a>, value: &Value<'a>) -> Result<bool, value::Error> {
        let stream: &Stream = value.try_unwrap_ref()?;
        Ok(stream.clone().all(|element| self.0.test(dye, &element))?)
    }
}

struct Empty;
impl PredicatePlan for Empty {
    fn test<'a>(&'a self, _dye: &'a DynEnv<'a>, value: &Value<'a>) -> Result<bool, value::Error> {
        let stream: &Stream = value.try_unwrap_ref()?;
        Ok(if let None = stream.clone().next()? { false } else { true })
    }
}

struct NonEmpty;
impl PredicatePlan for NonEmpty {
    fn test<'a>(&'a self, _dye: &'a DynEnv<'a>, value: &Value<'a>) -> Result<bool, value::Error> {
        let stream: &Stream = value.try_unwrap_ref()?;
        Ok(if let None = stream.clone().next()? { true } else { false })
    }
}
