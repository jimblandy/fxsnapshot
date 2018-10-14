use super::StaticError;
use super::ast::{Expr, LambdaId, UseId, Var};
use super::walkers::ExprWalker;

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::mem::replace;

/// An identifier for a lexical variable.
///
/// A value `VarNum { lambda, index }` refers to the `index`'th formal parameter
/// of the lambda expression whose id is `lambda`.
#[derive(Clone, Copy, Eq, PartialEq, Hash)]
struct VarAddr {
    lambda: LambdaId,
    index: usize,
}

impl fmt::Debug for VarAddr {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(fmt, "{:?}[{:?}]", self.lambda, self.index)
    }
}

#[derive(Debug, Default)]
struct CaptureMap<'expr> {
    /// The parameter lists of the lambdas currently in scope at this point in
    /// the traversal. Outer lambdas appear before inner lambdas.
    scopes: Vec<(LambdaId, &'expr Vec<String>)>,

    /// A map from each lambda to the set of variables that occur free in its
    /// body.
    lambdas: HashMap<LambdaId, HashSet<VarAddr>>,

    /// A map from each variable use to the variable it refers to.
    uses: HashMap<UseId, VarAddr>,

    /// The set of variables we've seen used so far within the innermost lambda
    /// at this point in the traversal.
    free: HashSet<VarAddr>
}

impl<'e> CaptureMap<'e> {
    fn new() -> CaptureMap<'e> {
        CaptureMap::default()
    }

    /// If there is a variable with the given `name` in scope, return its
    /// address. Otherwise, return `None`.
    fn find_var(&self, name: &str) -> Option<VarAddr> {
        for &(lambda_id, ref formals) in self.scopes.iter().rev() {
            if let Some(index) = formals.iter().position(|s| s == name) {
                return Some(VarAddr { lambda: lambda_id, index });
            }
        }
        None
    }
}

impl<'expr> ExprWalker<'expr> for CaptureMap<'expr> {
    type Error = StaticError;

    fn visit_expr(&mut self, expr: &'expr Expr) -> Result<(), StaticError> {
        match expr {
            &Expr::Var(Var::Lexical { id, ref name }) => {
                eprintln!("visiting use {:?}", id);
                if let Some(addr) = self.find_var(name) {
                    self.uses.insert(id, addr);
                    self.free.insert(addr);
                } else {
                    return Err(StaticError::UnboundVar { name: name.to_owned() });
                }
                Ok(())
            }
            &Expr::Lambda { id, ref formals, .. } => {
                // When we recurse, we want to find the set of free
                // variables for this lambda alone. Create a fresh `HashSet`,
                // and drop it in as our `free` while we walk this lambda's
                // body. We'll union its contents into our enclosing lambda's
                // free set when we're done.
                let parent_free = replace(&mut self.free, HashSet::new());

                // Add this lambda's formals to the current list of scopes,
                // so references in the lambda's body can see them.
                self.scopes.push((id, formals));

                eprintln!("Entering body of {:?}: {:?}", id, self);

                // Process the body of this lambda.
                self.visit_expr_children(expr)?;

                // Pop our formals off the list of scopes.
                self.scopes.pop();

                // References to this lambda's formals within its body are not
                // 'free', so drop them.
                self.free.retain(|addr| addr.lambda != id);

                // Take out our free set, and put the parent's back in place.
                let free = replace(&mut self.free, parent_free);

                // Include this lambda's free variables in the parent's set.
                self.free.extend(&free);

                // Record this lambda's free set.
                self.lambdas.insert(id, free);

                // kthx
                Ok(())
            }
            other => self.visit_expr_children(other)
        }
    }
}

pub fn debug_captures(expr: &Expr) {
    let mut capture_map = CaptureMap::new();
    capture_map.visit_expr(expr)
        .expect("error mapping captures");
    eprintln!("{:#?}", capture_map);
}
