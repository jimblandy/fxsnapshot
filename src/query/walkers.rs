//! Traits for traversing `Expr` trees.

use super::ast::{Expr, Predicate};

/// A type that can walk a query and accumulate information about it.
///
/// At the moment we're only using this for mapping captured variables, so it
/// includes only the features needed for that. But ideally, this would have all
/// sorts of methods for walk different kinds of nodes, with default
/// definitions that do nothing. Users could override the methods for the sorts
/// of nodes they care about.
///
/// Also, there's more than one way to walk a tree, so consider dividing this
/// into several traits that walk and propagate values in whatever way makes the
/// users clearest, rather than letting it slouch into something too neutral.
pub trait Walker<'e> {
    type Error;

    fn walk_expr(&mut self, expr: &'e Expr) -> Result<(), Self::Error> {
        expr.walk_children(self)
    }

    fn walk_predicate(&mut self, predicate: &'e Predicate) -> Result<(), Self::Error> {
        predicate.walk_children(self)
    }
}

/// A variant of the `Walker` trait that mutates the expression or predicate.
pub trait WalkerMut<'e> {
    type Error;

    fn walk_expr(&mut self, expr: &'e mut Expr) -> Result<(), Self::Error> {
        expr.walk_children_mut(self)
    }

    fn walk_predicate(&mut self, predicate: &'e mut Predicate) -> Result<(), Self::Error> {
        predicate.walk_children_mut(self)
    }
}

/// A type that a `Walker` or `WalkerMut` can walk, like `Expr` or `Predicate`.
pub trait Walkable {
    fn walk_children<'e, W: Walker<'e> + ?Sized>(&'e self, walker: &mut W) -> Result<(), W::Error>;
    fn walk_children_mut<'e, W: WalkerMut<'e> + ?Sized>(&'e mut self, walker: &mut W) -> Result<(), W::Error>;
}

macro_rules! walk_expr_body {
    ($expr:ident, $walker:ident) => {
        {
            match $expr {
                Expr::StreamLiteral(elts) => {
                    for elt in elts {
                        $walker.walk_expr(elt)?;
                    }
                }

                Expr::PredicateOp { stream, predicate, .. } => {
                    $walker.walk_expr(stream)?;
                    $walker.walk_predicate(predicate)?;
                }

                Expr::App { arg, fun } => {
                    $walker.walk_expr(arg)?;
                    $walker.walk_expr(fun)?;
                }

                Expr::Lambda { body, .. } => {
                    $walker.walk_expr(body)?;
                }

                Expr::Number(_) => (),
                Expr::String(_) => (),
                Expr::Var(_) => (),
            };
            Ok(())
        }
    }
}

impl Walkable for Expr {
    fn walk_children<'e, W: Walker<'e> + ?Sized>(&'e self, walker: &mut W) -> Result<(), W::Error> {
        walk_expr_body!(self, walker)
    }

    fn walk_children_mut<'e, W: WalkerMut<'e> + ?Sized>(&'e mut self, walker: &mut W) -> Result<(), W::Error> {
        walk_expr_body!(self, walker)
    }
}

macro_rules! walk_predicate_body {
    ($predicate:ident, $walker:ident) => {
        match $predicate {
            Predicate::Expr(expr) => $walker.walk_expr(expr),
            Predicate::Field(_name, subpred) => $walker.walk_predicate(subpred),
            Predicate::Regex(_) => Ok(()),

            Predicate::Ends(subpred) |
            Predicate::Any(subpred) |
            Predicate::All(subpred) |
            Predicate::Not(subpred) =>
                $walker.walk_predicate(subpred),

            Predicate::And(subpreds) |
            Predicate::Or(subpreds) => {
                for subpred in subpreds {
                    $walker.walk_predicate(subpred)?;
                }
                Ok(())
            },
        }
    }
}

impl Walkable for Predicate {
    fn walk_children<'e, W: Walker<'e> + ?Sized>(&'e self, walker: &mut W) -> Result<(), W::Error> {
        walk_predicate_body!(self, walker)
    }

    fn walk_children_mut<'e, W: WalkerMut<'e> + ?Sized>(&'e mut self, walker: &mut W) -> Result<(), W::Error> {
        walk_predicate_body!(self, walker)
    }
}
