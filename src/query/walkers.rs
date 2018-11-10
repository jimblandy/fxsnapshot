//! Traits for traversing `Expr` trees.

use super::ast::{Expr, Predicate};

macro_rules! define_walker_trait {
    ($trait:ident, $( $mut:tt )*) => (
        /// A type that can walk an expression and accumulate information about it.
        ///
        /// At the moment we're only using this for lambdas, so it includes only the
        /// features needed for that. But ideally, this would have all sorts of methods
        /// for visiting different kinds of nodes, with default definitions that do
        /// nothing. Users could override the methods for the sorts of nodes they care
        /// about.
        ///
        /// Also, there's more than one way to walk a tree, so consider dividing this
        /// into several traits that walk and propagate values in whatever way makes the
        /// users clearest, rather than letting it slouch into something too neutral.
        pub trait $trait<'e> {
            type Error;

            fn visit_expr(&mut self, expr: &'e $($mut)* Expr) -> Result<(), Self::Error> {
                self.visit_expr_children(expr)
            }

            fn visit_expr_children(&mut self, expr: &'e $($mut)* Expr)
                                   -> Result<(), Self::Error>
            {
                match expr {
                    Expr::StreamLiteral(elts) => {
                        for elt in elts {
                            self.visit_expr(elt)?;
                        }
                    }

                    Expr::PredicateOp { stream, predicate, .. } => {
                        self.visit_expr(stream)?;
                        self.visit_predicate(predicate)?;
                    }

                    Expr::App { arg, fun } => {
                        self.visit_expr(arg)?;
                        self.visit_expr(fun)?;
                    }

                    Expr::Lambda { body, .. } => {
                        self.visit_expr(body)?;
                    }

                    Expr::Number(_) => (),
                    Expr::String(_) => (),
                    Expr::Var(_) => (),
                }
                Ok(())
            }

            fn visit_predicate(&mut self, predicate: &'e $($mut)* Predicate)
                               -> Result<(), Self::Error>
            {
                self.visit_predicate_children(predicate)
            }

            fn visit_predicate_children(&mut self, predicate: &'e $($mut)* Predicate)
                                        -> Result<(), Self::Error>
            {
                match predicate {
                    Predicate::Expr(expr) => self.visit_expr(expr),
                    Predicate::Field(_name, subpred) => self.visit_predicate(subpred),
                    Predicate::Ends(subpred) | Predicate::Any(subpred) |
                    Predicate::All(subpred) | Predicate::Not(subpred) =>
                        self.visit_predicate(subpred),
                    Predicate::Regex(_) => Ok(()),
                    Predicate::And(subpreds) | Predicate::Or(subpreds) => {
                        for subpred in subpreds {
                            self.visit_predicate(subpred)?;
                        }
                        Ok(())
                    },
                }
            }
        }
    );
    ($trait:ident) => ( define_walker_trait!($trait,); );
}

define_walker_trait!(ExprWalker);
define_walker_trait!(ExprWalkerMut, mut);
