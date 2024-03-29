//! Types representing query execution strategies.
//!
//! The [`plan_expr`][pe] function takes an `Expr` and produces a `Plan` for
//! evaluating it.
//!
//! [pe]: fn.plan_expr.html

use fallible_iterator::{self, FallibleIterator};

use super::ast::{Expr, LambdaId, Predicate, PredicateOp, Var};
use super::breadth_first::{BreadthFirst, Step};
use super::fun::{CaptureList, StaticAnalysis, plan_lexical, plan_activation, plan_lambda};
use super::Activation;
use super::Context;
use super::value::{self, Callable, EvalResult, Function, Stream, TryUnwrap, Value};
use super::{Plan, PredicatePlan};
use crate::dump::{Edge, Node, NodeId};

use std::borrow::Cow;
use std::fmt;
use std::iter::once;
use std::rc::Rc;

/// Given the expression `expr`, return a `Plan` that will evaluate it.
pub fn plan_expr(expr: &Expr, analysis: &StaticAnalysis) -> Box<dyn Plan> {
    match expr {
        Expr::Number(n) => Box::new(Const(*n)),
        Expr::String(s) => Box::new(Const(s.clone())),
        Expr::StreamLiteral(elts) => {
            Box::new(StreamLiteral(elts.iter().map(|b| plan_expr(b, analysis)).collect()))
        }
        Expr::PredicateOp { id, stream, op, predicate } => plan_stream(*id, op, stream, predicate, analysis),

        Expr::Var(var) => plan_var(var, analysis),
        Expr::Lambda { id, formals, body } => plan_lambda(*id, formals, body, analysis),
        Expr::App { arg, fun } => plan_app(arg, fun, analysis),
    }
}

fn plan_var(var: &Var, analysis: &StaticAnalysis) -> Box<dyn Plan> {
    match var {
        Var::Root => Box::new(Root),
        Var::Nodes => Box::new(Nodes),
        Var::Map => Box::new(Map),
        Var::Lexical { id, name } => plan_lexical(*id, name, analysis),
        _ => unimplemented!("plan_var"),
    }
}

fn plan_app(arg: &Expr, fun: &Expr, analysis: &StaticAnalysis) -> Box<dyn Plan> {
    let arg_plan = plan_expr(arg, analysis);

    // Handle direct applications of certain built-in functions.
    match fun {
        Expr::Var(Var::Edges) => Box::new(Edges(arg_plan)),
        Expr::Var(Var::First) => Box::new(First(arg_plan)),
        Expr::Var(Var::Paths) => Box::new(Paths(arg_plan)),
        _ => {
            let fun_plan = plan_expr(fun, analysis);
            plan_activation(arg_plan, fun_plan)
        }
    }
}

fn plan_stream(id: LambdaId,
               op: &PredicateOp,
               stream: &Expr,
               predicate: &Predicate,
               analysis: &StaticAnalysis)
               -> Box<dyn Plan> {
    //let stream_plan = plan_expr(stream);
    //let predicate_plan = plan_predicate(predicate);
    match op {
        PredicateOp::Find => unimplemented!("PredicateOp::Find"),
        PredicateOp::Filter => plan_filter(id, stream, predicate, analysis),
        PredicateOp::Until => unimplemented!("PredicateOp::Until"),
    }
}

fn plan_filter(id: LambdaId, stream: &Expr, predicate: &Predicate, analysis: &StaticAnalysis) -> Box<dyn Plan> {
    let stream_plan: Box<dyn Plan>;
    let predicate_plan;

    // Can we implement `nodes { id: ... }` using `NodesById`, rather than a
    // linear search over all nodes?
    match stream {
        Expr::Var(Var::Nodes) => {
            if let Some((id, remainder)) = find_predicate_required_id(predicate) {
                stream_plan = Box::new(NodesById(plan_expr(id, analysis)));
                predicate_plan = plan_junction::<And>(&remainder, analysis);
            } else {
                stream_plan = Box::new(Nodes);
                predicate_plan = plan_predicate(predicate, analysis);
            }
        }
        stream => {
            stream_plan = plan_expr(stream, analysis);
            predicate_plan = plan_predicate(predicate, analysis);
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
        PlanOrTrivial::Plan(plan) => Box::new(Filter {
            stream: stream_plan,
            capture_list: analysis.get_capture_list(id),
            filter: plan.into(),
        }),
    }
}

/// When we plan a predicate, sometimes we discover that the predicate is always
/// true or always false, and we shouldn't produce an execution plan for it at
/// all. Values of this type are the results of such an effort: either a plan
/// for executing a predicate, or the answer we know it will always return.
enum PlanOrTrivial {
    Plan(Box<dyn PredicatePlan>),
    Trivial(bool),
}

impl PlanOrTrivial {
    fn map_plan<F>(self, f: F) -> PlanOrTrivial
    where
        F: FnOnce(Box<dyn PredicatePlan>) -> Box<dyn PredicatePlan>,
    {
        match self {
            PlanOrTrivial::Plan(plan) => PlanOrTrivial::Plan(f(plan)),
            trivial @ PlanOrTrivial::Trivial(_) => trivial,
        }
    }
}

fn plan_predicate(predicate: &Predicate, analysis: &StaticAnalysis) -> PlanOrTrivial {
    use self::PlanOrTrivial::*;
    match predicate {
        Predicate::Expr(expr) => Plan(Box::new(EqualPredicate(plan_expr(expr, analysis)))),
        Predicate::Field(field_name, sub) => plan_predicate(sub, analysis).map_plan(|predicate| {
            Box::new(FieldPredicate {
                field_name: field_name.clone(),
                predicate,
            })
        }),
        Predicate::Ends(sub) => plan_predicate(sub, analysis).map_plan(|subplan| Box::new(Ends(subplan))),
        Predicate::Any(sub) => match plan_predicate(sub, analysis) {
            PlanOrTrivial::Plan(p) => Plan(Box::new(Any(p))),
            PlanOrTrivial::Trivial(false) => Trivial(false),
            PlanOrTrivial::Trivial(true) => Plan(Box::new(NonEmpty)),
        },
        Predicate::All(sub) => match plan_predicate(sub, analysis) {
            PlanOrTrivial::Plan(p) => Plan(Box::new(All(p))),
            PlanOrTrivial::Trivial(true) => Trivial(true),
            PlanOrTrivial::Trivial(false) => Plan(Box::new(Empty)),
        },
        Predicate::Regex(regex) => Plan(Box::new(Regex((&**regex).clone()))),
        Predicate::And(predicates) => plan_junction::<And>(predicates, analysis),
        Predicate::Or(predicates) => plan_junction::<Or>(predicates, analysis),
        Predicate::Not(predicate) => match plan_predicate(predicate, analysis) {
            Trivial(k) => Trivial(!k),
            Plan(p) => Plan(Box::new(Not(p))),
        },
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
fn find_predicate_required_id(predicate: &Predicate) -> Option<(&Expr, Vec<Predicate>)> {
    match predicate {
        Predicate::Field(name, id_predicate) if name == "id" => {
            if let Predicate::Expr(id_expr) = &**id_predicate {
                return Some((id_expr, vec![]));
            }
        }

        Predicate::And(predicates) => {
            // Search the sub-predicates of this conjunction for one that
            // requires a specific id.
            if let Some((i, id, child_remainder)) =
                predicates.iter().enumerate().find_map(|(i, p)| {
                    find_predicate_required_id(p)
                        .map(|(id, child_remainder)| (i, id, child_remainder))
                }) {
                // predicates[i] requires a specific id. We've hoisted out the
                // id expression, so replace predicates[i] with child_remainder.
                return Some((id, splice(predicates, i, child_remainder)));
            }
        }

        // We could also look into conjunctions, to see if any sub-predicate
        // requires a specific id. The nice code for this uses find_map, which
        // isn't stable yet.
        _ => (),
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

#[derive(Debug)]
struct Const<T: fmt::Debug>(T);

impl<T: fmt::Debug> Plan for Const<T>
where
    T: Clone,
    for<'a> Value<'a>: From<T>,
{
    fn run<'a, 'd>(&self, _act: &'a Activation<'a, 'd>, _cx: &Context<'d>) -> EvalResult<'d> {
        Ok(Value::from(self.0.clone()))
    }
}

#[derive(Debug)]
struct StreamLiteral(Vec<Box<dyn Plan>>);

impl Plan for StreamLiteral {
    fn run<'a, 'd>(&self, act: &'a Activation<'a, 'd>, cx: &Context<'d>) -> EvalResult<'d> {
        let values: Vec<_> = self.0.iter().map(|p| p.run(act, cx)).collect();
        let iter = fallible_iterator::convert(values.into_iter());
        Ok(Value::from(Stream::new(iter)))
    }
}

#[derive(Debug)]
struct First(Box<dyn Plan>);
impl Plan for First {
    fn run<'a, 'd>(&self, act: &'a Activation<'a, 'd>, cx: &Context<'d>) -> EvalResult<'d> {
        let value = self.0.run(act, cx)?;
        let mut stream: Stream = value.try_unwrap()?;
        match stream.next()? {
            Some(v) => Ok(v),
            None => Err(value::Error::EmptyStream),
        }
    }
}

#[derive(Debug)]
struct Root;
impl Plan for Root {
    fn run<'a, 'd>(&self, _act: &'a Activation<'a, 'd>, cx: &Context<'d>) -> EvalResult<'d> {
        Ok(Value::from(cx.dump.get_root()))
    }
}

#[derive(Debug)]
struct Nodes;
impl Plan for Nodes {
    fn run<'a, 'd>(&self, _act: &'a Activation<'a, 'd>, cx: &Context<'d>) -> EvalResult<'d> {
        let iter = fallible_iterator::convert(cx.dump.nodes().map(|n| Ok(n.into())));
        Ok(Value::from(Stream::new(iter)))
    }
}

#[derive(Debug)]
struct NodesById(Box<dyn Plan>);
impl Plan for NodesById {
    fn run<'a, 'd>(&self, act: &'a Activation<'a, 'd>, cx: &Context<'d>) -> EvalResult<'d> {
        let value = self.0.run(act, cx)?;
        let id = NodeId(value.try_unwrap()?);
        let optional_node = cx.dump.get_node(id).map(|on| Ok(Value::from(on)));
        let iter = fallible_iterator::convert(optional_node.into_iter());
        Ok(Value::from(Stream::new(iter)))
    }
}

#[derive(Debug)]
struct Edges(Box<dyn Plan>);
impl Plan for Edges {
    fn run<'a, 'd>(&self, act: &'a Activation<'a, 'd>, cx: &Context<'d>) -> EvalResult<'d> {
        let value = self.0.run(act, cx)?;
        let node: &Node = value.try_unwrap()?;
        let iter = node.edges.iter().map(|e| Ok(Value::from(e)));
        let iter = fallible_iterator::convert(iter);
        Ok(Value::from(Stream::new(iter)))
    }
}

#[derive(Debug)]
struct Filter {
    stream: Box<dyn Plan>,
    capture_list: CaptureList,
    filter: Rc<dyn PredicatePlan>,
}

impl Plan for Filter {
    fn run<'a, 'd>(&self, act: &'a Activation<'a, 'd>, cx: &Context<'d>) -> EvalResult<'d> {
        let value = self.stream.run(act, cx)?;
        let stream: Stream = value.try_unwrap()?;

        // Gather up owned versions of everything filter's argument needs.
        let captured = act.get_captured(&self.capture_list);
        let filter = self.filter.clone();
        let cx = cx.clone();

        // The `move` closure here takes ownership of all the parts it needs, so
        // the stream becomes independent of this frame.
        let iter = stream.filter(move |item| {
            filter.test(item, &Activation::from_captured(&captured), &cx)
        });
        Ok(Value::from(Stream::new(iter)))
    }
}

#[derive(Debug)]
struct Paths(Box<dyn Plan>);
impl Plan for Paths {
    fn run<'a, 'd>(&self, act: &'a Activation<'a, 'd>, cx: &Context<'d>) -> EvalResult<'d> {
        let value = self.0.run(act, cx)?;
        let mut traversal = BreadthFirst::new(cx.dump);
        match value {
            Value::Node(node) => traversal.add_start_node(node.id),
            Value::Stream(mut stream) => {
                while let Some(elt) = stream.next()? {
                    let node: &Node<'a> = elt.try_unwrap()?;
                    traversal.add_start_node(node.id);
                }
            }
            other => {
                return Err(value::Error::Type {
                    expected: "node or stream of nodes",
                    actual: other.type_name(),
                })
            }
        };

        // The traversal produces a stream of paths, where each path is
        // itself a stream of alternating nodes and edges.
        let dump = cx.dump;
        let paths_iter = traversal
            .filter_map(move |path| {
                if path.is_empty() {
                    None
                } else {
                    // "Don't be too proud of this technological terror you've constructed."
                    let start = dump.get_node(path[0].origin).unwrap();
                    let iter = once(Value::from(start))
                        .chain(path.into_iter().flat_map(move |Step { edge, .. }| {
                            // If this edge is participating in a path, it
                            // must have a referent...
                            let referent = dump.get_node(edge.referent.unwrap()).unwrap();
                            once(Value::from(edge)).chain(once(Value::from(referent)))
                        })).map(Ok);
                    Some(Stream::new(fallible_iterator::convert(iter)))
                }
            })
            .map(Value::from)
            .map(Ok);
        Ok(Value::from(Stream::new(fallible_iterator::convert(
            paths_iter,
        ))))
    }
}

#[derive(Debug)]
struct Map;

// This is a bit weird: we use `Map` for both the plan that returns the function
// and the primitive `Function` itself.
impl<'dump> Callable<'dump> for Map {
    fn call_exact_arity(&self, actuals: &[Value<'dump>], cx: &Context<'dump>)
                        -> EvalResult<'dump>
    {
        assert_eq!(actuals.len(), 2);
        let stream: &Stream = actuals[0].try_unwrap_ref()?;
        let fun: &Function = actuals[1].try_unwrap_ref()?;

        // Owned versions of the above, for the closure to capture.
        let stream = stream.clone();
        let fun = fun.clone();
        let cx = cx.clone();

        let iter = stream.map(move |item| fun.call(&[item], &cx));
        Ok(Value::from(Stream::new(iter)))
    }

    fn arity(&self) -> usize {
        2
    }

    fn name(&self) -> Cow<str> {
        Cow::Borrowed("map")
    }
}

impl Plan for Map {
    fn run<'a, 'd>(&self, _act: &'a Activation<'a, 'd>, _cx: &Context<'d>) -> EvalResult<'d> {
        Ok(Value::from(Function::new(Map)))
    }
}

#[derive(Debug)]
struct EqualPredicate(Box<dyn Plan>);
impl PredicatePlan for EqualPredicate {
    fn test<'a, 'd>(&self, value: &Value<'d>, act: &Activation<'a, 'd>, cx: &Context<'d>) -> Result<bool, value::Error> {
        let given = self.0.run(act, cx)?;
        Ok(*value == given)
    }
}

#[derive(Debug)]
struct FieldPredicate {
    field_name: String,
    predicate: Box<dyn PredicatePlan>,
}
impl PredicatePlan for FieldPredicate {
    fn test<'a, 'd>(&self, value: &Value<'d>, act: &Activation<'a, 'd>, cx: &Context<'d>) -> Result<bool, value::Error> {
        let field = match value {
            Value::Node(node) => get_node_field(node, &self.field_name)?,
            Value::Edge(edge) => get_edge_field(edge, &self.field_name)?,
            _ => {
                return Err(value::Error::Type {
                    expected: "node or edge",
                    actual: value.type_name(),
                });
            }
        };
        field.map_or(Ok(false), |field_value| {
            self.predicate.test(&field_value, act, cx)
        })
    }
}

fn get_node_field<'v>(node: &'v Node, field: &str) -> Result<Option<Value<'v>>, value::Error> {
    Ok(match field {
        "id" => Some(node.id.0.into()),
        "size" => node.size.map(Value::from),
        "coarseType" => Some(String::from(node.coarseType).into()),
        "typeName" => node.typeName.map(|t| t.to_string().into()),
        "JSObjectClassName" => node.JSObjectClassName.map(|t| t.to_string().into()),
        "scriptFilename" => node.scriptFilename.map(|t| t.to_string().into()),
        "descriptiveTypeName" => node.descriptiveTypeName.map(|t| t.to_string().into()),
        _ => {
            return Err(value::Error::NoSuchField {
                value_type: "nodes",
                field: field.into(),
            })
        }
    })
}

fn get_edge_field<'v>(edge: &'v Edge, field: &str) -> Result<Option<Value<'v>>, value::Error> {
    Ok(match field {
        "referent" => edge.referent.map(|id| Value::from(id.0)),
        "name" => edge.name.map(|n| n.to_string().into()),
        _ => {
            return Err(value::Error::NoSuchField {
                value_type: "edges",
                field: field.into(),
            })
        }
    })
}

#[derive(Debug)]
struct Ends(Box<dyn PredicatePlan>);
impl PredicatePlan for Ends {
    fn test<'a, 'd>(&self, value: &Value<'d>, act: &Activation<'a, 'd>, cx: &Context<'d>) -> Result<bool, value::Error> {
        let stream: &Stream = value.try_unwrap_ref()?;
        let last = stream.clone().last()?.ok_or(value::Error::EmptyStream)?;
        self.0.test(&last, act, cx)
    }
}

#[derive(Debug)]
struct Regex(regex::Regex);
impl PredicatePlan for Regex {
    fn test<'a, 'd>(&self, value: &Value<'d>, _act: &Activation<'a, 'd>, _cx: &Context<'d>) -> Result<bool, value::Error> {
        let string: &String = value.try_unwrap_ref()?;
        Ok(self.0.is_match(&string))
    }
}

#[derive(Debug)]
struct And(Vec<Box<dyn PredicatePlan>>);
impl PredicatePlan for And {
    fn test<'a, 'd>(&self, value: &Value<'d>, act: &Activation<'a, 'd>, cx: &Context<'d>) -> Result<bool, value::Error> {
        fallible_iterator::convert(self.0.iter().map(Ok)).all(|plan| plan.test(value, act, cx))
    }
}

#[derive(Debug)]
struct Or(Vec<Box<dyn PredicatePlan>>);
impl PredicatePlan for Or {
    fn test<'a, 'd>(&self, value: &Value<'d>, act: &Activation<'a, 'd>, cx: &Context<'d>) -> Result<bool, value::Error> {
        fallible_iterator::convert(self.0.iter().map(Ok)).any(|plan| plan.test(value, act, cx))
    }
}

#[derive(Debug)]
struct Not(Box<dyn PredicatePlan>);
impl PredicatePlan for Not {
    fn test<'a, 'd>(&self, value: &Value<'d>, act: &Activation<'a, 'd>, cx: &Context<'d>) -> Result<bool, value::Error> {
        Ok(!self.0.test(value, act, cx)?)
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
    fn construct(subplans: Vec<Box<dyn PredicatePlan>>) -> Box<dyn PredicatePlan>;
}

impl BlahJunction for And {
    const CONSONANT: bool = true;
    fn construct(subplans: Vec<Box<dyn PredicatePlan>>) -> Box<dyn PredicatePlan> {
        Box::new(And(subplans))
    }
}

impl BlahJunction for Or {
    const CONSONANT: bool = false;
    fn construct(subplans: Vec<Box<dyn PredicatePlan>>) -> Box<dyn PredicatePlan> {
        Box::new(Or(subplans))
    }
}

/// Plan a `BlahJunction` predicate with the given `subterms`.
fn plan_junction<J: BlahJunction>(subterms: &[Predicate], analysis: &StaticAnalysis) -> PlanOrTrivial {
    let mut plans = Vec::new();
    for pot in subterms.iter().map(|subterm| plan_predicate(subterm, analysis)) {
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

#[derive(Debug)]
struct Any(Box<dyn PredicatePlan>);
impl PredicatePlan for Any {
    fn test<'a, 'd>(&self, value: &Value<'d>, act: &Activation<'a, 'd>, cx: &Context<'d>) -> Result<bool, value::Error> {
        let stream: &Stream = value.try_unwrap_ref()?;
        Ok(stream.clone().any(|element| self.0.test(&element, act, cx))?)
    }
}

#[derive(Debug)]
struct All(Box<dyn PredicatePlan>);
impl PredicatePlan for All {
    fn test<'a, 'd>(&self, value: &Value<'d>, act: &Activation<'a, 'd>, cx: &Context<'d>) -> Result<bool, value::Error> {
        let stream: &Stream = value.try_unwrap_ref()?;
        Ok(stream.clone().all(|element| self.0.test(&element, act, cx))?)
    }
}

#[derive(Debug)]
struct Empty;
impl PredicatePlan for Empty {
    fn test<'a, 'd>(&self, value: &Value<'d>, _act: &Activation<'a, 'd>, _cx: &Context<'d>) -> Result<bool, value::Error> {
        let stream: &Stream = value.try_unwrap_ref()?;
        Ok(stream.clone().next()?.is_none())
    }
}

#[derive(Debug)]
struct NonEmpty;
impl PredicatePlan for NonEmpty {
    fn test<'a, 'd>(&self, value: &Value<'d>, _act: &Activation<'a, 'd>, _cx: &Context<'d>) -> Result<bool, value::Error> {
        let stream: &Stream = value.try_unwrap_ref()?;
        Ok(stream.clone().next()?.is_some())
    }
}
