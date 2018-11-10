use id_vec::IdVec;
use super::{Context, EvalResult, Plan, StaticError, Value};
use super::ast::{Expr, LambdaId, UseId, Var};
use super::run::plan_expr;
use super::value::{Callable, Error, Function};
use super::walkers::{ExprWalker, ExprWalkerMut};

use std::collections::{HashMap, HashSet};
use std::borrow::Cow;
use std::fmt;
use std::iter::FromIterator;
use std::mem::replace;
use std::rc::Rc;

/// A `Function` created by evaluating a lambda expression.
#[derive(Clone)]
struct Closure<'a> {
    /// Information shared by all closures created from this lambda expression.
    lambda: Rc<LambdaExpr>,

    /// A vector of captured variables' values, referred to by `Captured` plans.
    /// Possibly borrowed by some stack frames, if we're running this closure at
    /// the moment.
    captured: Vec<Value<'a>>,
}

/// Information about a given lambda expression, shared by all closures created
/// by evaluating it.
#[derive(Debug)]
struct LambdaExpr {
    /// The name of this closure.
    name: String,

    /// The number of formal parameters it takes.
    arity: usize,

    /// An evaluation plan for the body of the closure.
    body: Box<Plan>,

    /// How to populate the `captured` vector of a `Closure` created for this
    /// `LambdaExpr`. In this vector, `captured[i]` is the location at which the
    /// value that belongs in `Closure::captured[i]` can be found in the
    /// `Activation` for the lexical context surrounding this lambda expression.
    captured: Vec<VarLocation>,
}

/// Data for a closure's activation. The details of this struct are private to
/// the function machinery: they are created by `Call` plans applying closures
/// created by `LambdaExprPlan`s, and consulted by `Actual` and `Captured` plans
/// to fetch variables' values.
pub struct Activation<'act, 'dump> {
    /// The closure we are currently executing. For evaluation, this points to a
    /// dummy `Closure`.
    closure: &'act Closure<'dump>,

    /// The actual parameters passed to this closure by the call. For
    /// evaluation, this is an empty slice.
    actuals: &'act [Value<'dump>],
}

/// Places a variable's value might live in an `Activation`.
#[derive(Clone, Copy)]
enum VarLocation {
    /// The value of the parameter with the given index.
    Actual(usize),

    /// The value at the given index in the current closure's `captured` vector.
    Captured(usize),
}

impl fmt::Debug for VarLocation {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        match self {
            VarLocation::Actual(i) => write!(fmt, "arg #{}", i),
            VarLocation::Captured(i) => write!(fmt, "cap #{}", i),
        }
    }
}

impl<'a, 'd> Activation<'a, 'd> {
    /// Create an activation suitable for an eval.
    pub fn for_eval(base: &'a ActivationBase<'d>) -> Activation<'a, 'd> {
        Activation {
            closure: &base.closure,
            actuals: &[],
        }
    }

    fn get(&self, loc: &VarLocation) -> Value<'d> {
        match loc {
            VarLocation::Actual(i) => self.actuals[*i].clone(),
            VarLocation::Captured(i) => self.closure.captured[*i].clone(),
        }
    }
}

#[derive(Debug)]
struct Crash(&'static str);
impl Plan for Crash {
    fn run<'a, 'd>(&self, _act: &'a Activation<'a, 'd>, _cx: &Context<'d>) -> EvalResult<'d> {
        panic!("{}", self.0);
    }
}

pub struct ActivationBase<'dump> {
    closure: Closure<'dump>
}

impl<'dump> ActivationBase<'dump> {
    pub fn from_context(_cx: &Context<'dump>) -> ActivationBase<'dump> {
        let body = Box::new(Crash("dummy ActivationBase closure should never be called"));
        let lambda = LambdaExpr {
            name: "dummy ActivationBase closure".to_string(),
            arity: 0,
            body,
            captured: vec![],
        };
        let closure = Closure {
            lambda: Rc::new(lambda),
            captured: vec![],
        };
        ActivationBase { closure }
    }
}

impl<'dump> Callable<'dump> for Closure<'dump> {
    fn call_exact_arity(&self, actuals: &[Value<'dump>], cx: &Context<'dump>)
                        -> EvalResult<'dump>
    {
        // Create a fresh activation to evaluate the body in, providing the
        // closure we're calling and the actual parameters it was passed.
        let actuals = actuals.to_owned();
        let activation = Activation {
            closure: self,
            actuals: &actuals,
        };
        self.lambda.body.run(&activation, cx)
    }

    fn arity(&self) -> usize {
        self.lambda.arity
    }

    fn name(&self) -> Cow<str> {
        Cow::Borrowed(&self.lambda.name)
    }
}

#[derive(Default)]
/// An `ExprWalkerMut` that assigns a distinct label to each node in the AST
/// that needs one, for the benefit of closure layout.
///
/// This assigns `LambdaId`s and `UseId`s starting at zero, with no gaps. You
/// can use the `ExprLabeler`'s `lambda_count` and `use_count` methods to get
/// the number of ids of each type that were assigned, to estimate table
/// capacities.
///
/// This assigns `LambdaId`s such that every lambda expression's id is greater
/// than its parent's. This means that iterating over `LambdaId`'s in numeric
/// order does a pre-order, depth-first traversal of the lambdas.
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

    fn lambda_count(&self) -> usize {
        self.next_lambda
    }

    fn use_count(&self) -> usize {
        self.next_use
    }
}

impl<'e> ExprWalkerMut<'e> for ExprLabeler {
    type Error = StaticError;
    fn visit_expr(&mut self, expr: &'e mut Expr) -> Result<(), StaticError> {
        match expr {
            Expr::Lambda { id, .. } => {
                *id = self.next_lambda();
            }
            Expr::Var(Var::Lexical { id, .. }) => {
                *id = self.next_use();
            }
            _ => (),
        }
        self.visit_expr_children(expr)
    }
}

/// An identifier for a lexical variable.
///
/// A value `VarNum { lambda, index }` refers to the `index`'th formal parameter
/// of the lambda expression whose id is `lambda`.
#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd, Hash)]
struct VarAddr {
    lambda: LambdaId,
    index: usize,
}

impl fmt::Debug for VarAddr {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(fmt, "{:?}[{:?}]", self.lambda, self.index)
    }
}

/// Information about a particular lambda expression.
#[derive(Debug)]
struct LambdaInfo {
    /// The number of formal parameters this lambda expects.
    arity: usize,

    /// The id of the immediately enclosing lambda, if any.
    parent: Option<LambdaId>,

    /// The set of variables this lambda captures.
    captured: HashSet<VarAddr>,
}

/// Information about a particular use of a variable.
#[derive(Debug, Eq, PartialEq)]
struct UseInfo {
    /// The lambda in which the use occurs, if any.
    lambda: Option<LambdaId>,

    /// The variable the use refers to.
    referent: VarAddr,
}

#[derive(Debug, Default)]
struct CaptureMap {
    /// Information about each lambda.
    lambdas: IdVec<LambdaId, LambdaInfo>,

    /// Information about each variable use.
    uses: IdVec<UseId, UseInfo>,
}

#[derive(Debug, Default)]
struct CaptureMapBuilder<'expr> {
    /// The CaptureMap we're building.
    map: CaptureMap,

    /// The parameter lists of the lambdas currently in scope at this point in
    /// the traversal. Outer lambdas appear before inner lambdas.
    scopes: Vec<(LambdaId, &'expr Vec<String>)>,

    /// The set of variables we've seen used so far within the innermost lambda
    /// at this point in the traversal.
    captured: HashSet<VarAddr>,
}

impl<'e> CaptureMapBuilder<'e> {
    fn new() -> CaptureMapBuilder<'e> {
        CaptureMapBuilder::default()
    }

    fn build(self) -> CaptureMap {
        // These should always be empty at the end of any traversal.
        assert!(self.scopes.is_empty());
        assert!(self.captured.is_empty());

        self.map
    }

    /// If there is a variable with the given `name` in scope, return its
    /// address. Otherwise, return `None`.
    fn find_var(&self, name: &str) -> Option<VarAddr> {
        for &(lambda_id, ref formals) in self.scopes.iter().rev() {
            if let Some(index) = formals.iter().position(|s| s == name) {
                return Some(VarAddr {
                    lambda: lambda_id,
                    index,
                });
            }
        }
        None
    }
}

impl<'expr> ExprWalker<'expr> for CaptureMapBuilder<'expr> {
    type Error = StaticError;

    fn visit_expr(&mut self, expr: &'expr Expr) -> Result<(), StaticError> {
        let enclosing = self.scopes.last().map(|(id, _)| *id);
        match expr {
            &Expr::Var(Var::Lexical { id, ref name }) => {
                if let Some(referent) = self.find_var(name) {
                    self.map.uses.push_at(id, UseInfo { lambda: enclosing, referent });
                    self.captured.insert(referent);
                } else {
                    return Err(StaticError::UnboundVar {
                        name: name.to_owned(),
                    });
                }
                Ok(())
            }
            &Expr::Lambda {
                id, ref formals, ..
            } => {
                let arity = formals.len();

                // Since `self.map.lambdas` is an `IdVec`, it must be built in
                // order of increasing id, so parents must come before children.
                // We need to create the entry for this lambda before we visit
                // its children. We can fill in the captured set afterwards.
                self.map.lambdas.push_at(id, LambdaInfo {
                    arity,
                    parent: enclosing,
                    captured: Default::default()
                });

                // When we recurse, we want to find the set of captured
                // variables for this lambda alone. Create a fresh `HashSet`,
                // and drop it in as our `captured` while we walk this lambda's
                // body. We'll union its contents into our enclosing lambda's
                // captured set when we're done.
                let parent_captured = replace(&mut self.captured, HashSet::new());

                // Add this lambda's formals to the current list of scopes,
                // so references in the lambda's body can see them.
                self.scopes.push((id, formals));

                // Process the body of this lambda.
                self.visit_expr_children(expr)?;

                // Pop our formals off the list of scopes.
                self.scopes.pop();

                // References to this lambda's formals within its body are not
                // 'captured', so drop them.
                self.captured.retain(|addr| addr.lambda != id);

                // Take out our captured set, and put the parent's back in place.
                let captured = replace(&mut self.captured, parent_captured);

                // Include this lambda's captured variables in the parent's set.
                self.captured.extend(&captured);

                // Record this lambda's captured set.
                self.map.lambdas[id].captured = captured;

                // kthx
                Ok(())
            }
            other => self.visit_expr_children(other),
        }
    }
}

/// For a given lambda, where to find the values of captured variables and
/// actual parameters it uses.
#[derive(Debug, Default)]
struct Layout {
    /// How to build the `captured` vector of a `Closure` for this lambda, from
    /// the information available in the lambda's lexical context.
    ///
    /// Each `to_capture[i]` says where to find the value that belongs in
    /// `captured[i]`. Note that, since this is describing how to build the
    /// closure, these are the homes those values occupy *outside* the lambda,
    /// not the homes they will have in the closure.
    captured: Vec<VarLocation>,

    /// A map from each variable that occurs free in this lambda's body to the
    /// location at which its value can be found in an `Activation` of that
    /// lambda.
    locations: HashMap<VarAddr, VarLocation>,
}

#[derive(Debug, Default)]
struct ClosureLayouts {
    /// The layout for each lambda.
    lambdas: IdVec<LambdaId, Layout>,

    /// For each variable use, where its value can be found in an `Activation`
    /// for its lambda.
    referents: IdVec<UseId, VarLocation>
}

impl ClosureLayouts {
    fn from_capture_map(cm: CaptureMap) -> ClosureLayouts {
        let mut layouts = ClosureLayouts::default();

        // First, lay out each lambda's closure. Visit parents before children,
        // so the children can use the parent's layout to find the variables
        // they need to capture.
        for (lambda, LambdaInfo { arity, parent, captured }) in cm.lambdas.into_iter().enumerate() {
            let lambda = LambdaId(lambda);
            let mut layout = Layout::default();

            // Our formals are available directly from the Activation.
            for index in 0..arity {
                let formal = VarAddr { lambda, index };
                layout.locations.insert(formal, VarLocation::Actual(index));
            }

            // Variables bound in outer lambdas must be captured when this
            // closure is created, and fetched from the closure's `captured`
            // vector at run time.
            if let Some(parent) = parent {
                // Get a map of where our enclosing lambda stashed all the
                // values we need.
                let parent_locations = &layouts.lambdas[parent].locations;

                // List the variables we capture. Sort to avoid being influenced
                // by the HashSet iteration order.
                let mut captured = Vec::from_iter(captured.into_iter());
                captured.sort();

                // Build the vector indicating where each captured variable can
                // be found in our parent's activation, and the map of where
                // they can be found in our activation.
                for addr in captured {
                    layout.locations.insert(addr, VarLocation::Captured(layout.captured.len()));
                    layout.captured.push(parent_locations[&addr]);
                }
            } else {
                // This is a top-level lambda, so it had better not have any
                // captured variables!
                assert!(captured.is_empty());
            }

            layouts.lambdas.push_at(lambda, layout);
        }

        // Now discover the location to which each variable use refers.
        for (use_id, UseInfo { lambda, referent }) in cm.uses.into_iter().enumerate() {
            let use_id = UseId(use_id);
            if let Some(lambda) = lambda {
                let location = layouts.lambdas[lambda].locations[&referent];
                layouts.referents.push_at(use_id, location);
            } else {
                unimplemented!("references to global variables");
            }
        }

        layouts
    }
}

/// Statically determined information needed for planning.
pub struct StaticAnalysis(ClosureLayouts);

pub fn static_analysis(expr: &mut Expr) -> Result<StaticAnalysis, StaticError> {
    // Label lambdas, variable uses, etc.
    ExprLabeler::new().visit_expr(expr)?;
    eprintln!("labeled expr: {:?}", expr);

    // Build a map of which variables are captured by which lambdas.
    let map = {
        let mut builder = CaptureMapBuilder::new();
        builder.visit_expr(expr)?;
        builder.build()
    };
    eprintln!("{:#?}", map);

    // Chose how each lambda's closure should be laid out, and then note the
    // location each variable reference now refers to.
    let layouts = ClosureLayouts::from_capture_map(map);
    Ok(StaticAnalysis(layouts))
}

/// A use of a captured variable's value.
#[derive(Debug)]
struct Captured(usize);
impl Plan for Captured {
    fn run<'a, 'd>(&self, act: &'a Activation<'a, 'd>, _cx: &Context<'d>) -> EvalResult<'d> {
        Ok(act.closure.captured[self.0].clone())
    }
}

/// A use of an argument passed to the closure.
#[derive(Debug)]
struct Actual(usize);
impl Plan for Actual {
    fn run<'a, 'd>(&self, act: &'a Activation<'a, 'd>, _cx: &Context<'d>) -> EvalResult<'d> {
        Ok(act.actuals[self.0].clone())
    }
}

pub fn plan_lexical(id: UseId, _name: &str, analysis: &StaticAnalysis) -> Box<Plan> {
    match analysis.0.referents[id] {
        VarLocation::Actual(i) => Box::new(Actual(i)),
        VarLocation::Captured(i) => Box::new(Captured(i)),
    }
}

#[derive(Debug)]
struct LambdaExprPlan(Rc<LambdaExpr>);

pub fn plan_lambda(id: LambdaId, formals: &[String], body: &Expr, analysis: &StaticAnalysis) -> Box<Plan> {
    let lambda = LambdaExpr {
        name: format!("anonymous {:?}", id),
        arity: formals.len(),
        body: plan_expr(body, analysis),
        captured: analysis.0.lambdas[id].captured.clone(),
    };
    Box::new(LambdaExprPlan(Rc::new(lambda)))
}

impl Plan for LambdaExprPlan {
    fn run<'a, 'd>(&self, act: &'a Activation<'a, 'd>, _cx: &Context<'d>) -> EvalResult<'d> {
        Ok(Value::Function(Function(Rc::new(Closure {
            lambda: self.0.clone(),
            captured: self.0.captured.iter().map(|loc| act.get(loc)).collect()
        }))))
    }
}

#[derive(Debug)]
struct Call {
    arg: Box<Plan>,
    fun: Box<Plan>
}

pub fn plan_activation(arg: Box<Plan>, fun: Box<Plan>) -> Box<Plan> {
    Box::new(Call { arg, fun })
}

impl Plan for Call {
    fn run<'a, 'd>(&self, act: &'a Activation<'a, 'd>, cx: &Context<'d>) -> EvalResult<'d> {
        let args = [self.arg.run(act, cx)?];
        let fun = match self.fun.run(act, cx)? {
            Value::Function(f) => f,
            _ => { return Err(Error::NotAFunction); }
        };

        fun.call(&args, cx)
    }
}


#[cfg(test)]
mod test {
    use super::{CaptureMap, CaptureMapBuilder, VarAddr};
    use query::ast::{Expr, LambdaId, UseId};
    use query::test_utils::*;
    use query::walkers::ExprWalker;
    use std::collections::{HashMap, HashSet};
    use std::iter::FromIterator;

    fn varaddr(lambda: usize, index: usize) -> VarAddr {
        VarAddr {
            lambda: LambdaId(lambda),
            index,
        }
    }

    fn make_capture_map(expr: &Expr) -> CaptureMap {
        let mut builder = CaptureMapBuilder::new();
        builder.visit_expr(expr)
            .expect("build capture map");
        builder.build()
    }

    #[test]
    fn trivial() {
        let expr = root();
        let cm = make_capture_map(&expr);
        assert!(cm.lambdas.is_empty());
        assert!(cm.uses.is_empty());
    }

    #[test]
    fn single_lambda() {
        let expr = lambda(70, &["x", "y", "z"], app(var(38, "y"), var(92, "z")));
        let cm = make_capture_map(&expr);
        assert_eq!(
            cm.lambdas,
            HashMap::from_iter(vec![
                       (LambdaId(70), HashSet::new()) // no free variables
                   ])
        );
        assert_eq!(
            cm.uses,
            HashMap::from_iter(vec![
                (UseId(38), varaddr(70, 1)),
                (UseId(92), varaddr(70, 2))
            ])
        );
    }

    #[test]
    fn two_lambdas() {
        let expr = lambda(
            208,
            &["x", "y"],
            lambda(193, &["z", "w"], app(var(215, "y"), var(50, "z"))),
        );
        let cm = make_capture_map(&expr);
        assert_eq!(
            cm.lambdas,
            HashMap::from_iter(vec![
                (LambdaId(208), HashSet::new()), // no free variables
                (LambdaId(193), HashSet::from_iter(vec![varaddr(208, 1)]))
            ])
        );
        assert_eq!(
            cm.uses,
            HashMap::from_iter(vec![
                (UseId(215), varaddr(208, 1)),
                (UseId(50), varaddr(193, 0))
            ])
        );
    }

    #[test]
    fn three_lambdas() {
        // |a,b| |c,d| b d (|a,d| (a b c d))
        let expr = lambda(
            152,
            &["a", "b"],
            lambda(
                30,
                &["c", "d"],
                app(
                    app(var(9, "b"), var(179, "d")),
                    lambda(
                        106,
                        &["a", "d"],
                        app(
                            app(app(var(89, "a"), var(109, "b")), var(57, "c")),
                            var(161, "d"),
                        ),
                    ),
                ),
            ),
        );
        let cm = make_capture_map(&expr);
        assert_eq!(
            cm.lambdas,
            HashMap::from_iter(vec![
                (LambdaId(152), HashSet::new()), // no free variables
                (LambdaId(30), HashSet::from_iter(vec![varaddr(152, 1)])),
                (
                    LambdaId(106),
                    HashSet::from_iter(vec![varaddr(152, 1), varaddr(30, 0)])
                ),
            ])
        );
        assert_eq!(
            cm.uses,
            HashMap::from_iter(vec![
                (UseId(9), varaddr(152, 1)),
                (UseId(179), varaddr(30, 1)),
                (UseId(89), varaddr(106, 0)),
                (UseId(109), varaddr(152, 1)),
                (UseId(57), varaddr(30, 0)),
                (UseId(161), varaddr(106, 1)),
            ])
        );
    }
}

