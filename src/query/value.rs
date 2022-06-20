use crate::dump::{Edge, Node};
use failure;
use fallible_iterator::FallibleIterator;
use std::cmp::PartialEq;
use std::borrow::Cow;
use std::io;
use std::rc::Rc;
use super::Context;
use super::stream;

/// A value produced by evaluating an expression.
///
/// Values of this type may borrow contents from a `CoreDump`, hence the `'dump`
/// lifetime parameter. Furthermore, stream values also values lazily, as
/// directed by a borrowed portion of the expression, hence the `'expr` lifetime
/// parameter. Almost certainly, a `Value` will not outlive either the
/// expression that produced it, or the dump that its evaluation consulted.
#[derive(Clone)]
pub enum Value<'a> {
    Number(u64),
    String(String),
    Edge(&'a Edge<'a>),
    Node(&'a Node<'a>),
    Stream(Stream<'a>),
    Function(Function<'a>),
}

pub type Stream<'a> = stream::Stream<'a, Value<'a>, Error>;

/// The result of evaluating an expression: either a value, or a
/// [`value::Error`](#type.Error).
pub type EvalResult<'a> = Result<Value<'a>, Error>;

#[derive(Clone)]
pub struct Function<'a>(pub Rc<dyn 'a + Callable<'a>>);

pub trait Callable<'dump> {
    /// Call `self`, passing the arguments given in `actuals`, running in the
    /// given context.
    ///
    /// Arguments appear in `actuals` from left to right: a call like `x y z f`
    /// passes a slice `&[x, y, z]`. Implementations may assume that the length
    /// of `actuals` matches `self.arity()`; callers are responsible for
    /// ensuring that this is the case.
    fn call_exact_arity(&self, actuals: &[Value<'dump>], cx: &Context<'dump>)
                        -> EvalResult<'dump>;

    /// Return the number of arguments this function expects. Every `Callable`s'
    /// arity is greater than zero; zero-arity functions don't work too well
    /// with our application syntax.
    fn arity(&self) -> usize;

    /// Return this function's name.
    fn name(&self) -> Cow<str>;
}

/// An error raised during expression evaluation.
#[derive(Clone, Fail, Debug)]
pub enum Error {
    /// Type mismatch.
    #[fail(display = "expected type {}, got {}", expected, actual)]
    Type {
        actual: &'static str,
        expected: &'static str,
    },

    /// Trying to draw a value (`first`, etc.) from an empty stream.
    #[fail(display = "stream produced no values")]
    EmptyStream,

    /// Matching on a non-existent Node or Edge field.
    #[fail(display = "{} have no field named {}", value_type, field)]
    NoSuchField {
        value_type: &'static str,
        field: String,
    },

    /// Attempt to apply a value that is not a function.
    #[fail(display = "attempt to apply value that is not a function")]
    NotAFunction,
}

/// `Value` implements `TryUnwrap<T>` for each type `T` it can be unwrapped
/// into.
pub trait TryUnwrap<T: Sized>: Sized {
    /// If `self` (which is always a `Value`) holds a `T`, return that as the
    /// success value. Otherwise, report a type error, using the type of `T` and
    /// the actual content of `self`
    fn try_unwrap(self) -> Result<T, Error>;

    /// Like try_unwrap, but for references to values. Returns a `Result` of a
    /// reference.
    fn try_unwrap_ref(&self) -> Result<&T, Error>;
}

/// How to lay out elements of a stream when printed: one per line, or
/// space-separated fields on one line.
enum Orientation {
    /// The sequence is laid out on a single line, with values separated by
    /// spaces. The `usize` indicates the depth of indentation of the entire line.
    Horizontal(usize),

    /// A sequence laid out as a series of lines. The `usize` indicates the
    /// depth of indentation applied to each element.
    Vertical(usize),
}

impl<'a> Value<'a> {
    pub fn top_write(&self, stream: &mut dyn io::Write) -> Result<(), failure::Error> {
        self.write(&Orientation::Vertical(0), stream)
    }

    /// Write `self` to `stream`. If it is a stream, lay it out using `orientation`.
    fn write(
        &self,
        orientation: &Orientation,
        stream: &mut dyn io::Write,
    ) -> Result<(), failure::Error> {
        match self {
            Value::Number(n) => write!(stream, "{}", n)?,
            Value::String(s) => write!(stream, "{}", s)?,
            Value::Edge(e) => write!(stream, "{:?}", e)?,
            Value::Node(n) => write!(stream, "{:?}", n)?,
            Value::Function(f) => write!(stream, "function {:?}", f.0.name())?,
            Value::Stream(s) => {
                return write_stream(s, orientation, stream);
            }
        }
        Ok(())
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Number(_) => "number",
            Value::String(_) => "string",
            Value::Edge(_) => "edge",
            Value::Node(_) => "node",
            Value::Stream(_) => "stream",
            Value::Function(_) => "function",
        }
    }
}

fn write_stream<'a>(
    stream: &Stream<'a>,
    orientation: &Orientation,
    output: &mut dyn io::Write,
) -> Result<(), failure::Error> {
    let mut stream = stream.clone();
    match orientation {
        Orientation::Horizontal(indent) => {
            // Any streams nested within this one should be laid out vertically,
            // and indented relative to us.
            let nested_orientation = Orientation::Vertical(indent + 4);
            write!(output, "[ ")?;
            let mut first = true;
            while let Some(value) = stream.next()? {
                if !first {
                    write!(output, " ")?;
                }
                value.write(&nested_orientation, output)?;
                first = false;
            }
            write!(output, " ]")?;
        }
        Orientation::Vertical(indent) => {
            // Any streams nested within this one should be laid out
            // horizontally, as a line of their own.
            let nested_orientation = Orientation::Horizontal(*indent);

            writeln!(output, "[")?;
            while let Some(value) = stream.next()? {
                write!(output, "{:1$}", "", indent)?;
                value.write(&nested_orientation, output)?;
                writeln!(output)?;
            }
            write!(output, "{:1$}]", "", indent)?;
        }
    }
    Ok(())
}

impl<'a, 'b> PartialEq<Value<'a>> for Value<'b> {
    fn eq(&self, other: &Value<'a>) -> bool {
        use self::Value::*;
        match (self, other) {
            (Number(left), Number(right)) => left == right,
            (String(left), String(right)) => left == right,
            (Edge(left), Edge(right)) => left == right,
            (Node(left), Node(right)) => left.id == right.id,
            _ => false,
        }
    }
}

trait TypeName {
    const NAME: &'static str;
}

macro_rules! impl_value_variant {
    // lifetime identifier hygiene, lol
    ($type:ty, $variant:ident, $name:tt) => {
        impl<'a> From<$type> for Value<'a> {
            fn from(v: $type) -> Value<'a> {
                Value::$variant(v)
            }
        }

        impl<'a> TypeName for $type {
            const NAME: &'static str = $name;
        }

        impl<'a> TryUnwrap<$type> for Value<'a> {
            fn try_unwrap(self) -> Result<$type, Error> {
                if let Value::$variant(v) = self {
                    Ok(v)
                } else {
                    Err(Error::Type {
                        expected: <$type>::NAME,
                        actual: self.type_name(),
                    })
                }
            }

            fn try_unwrap_ref(&self) -> Result<&$type, Error> {
                if let Value::$variant(v) = self {
                    Ok(v)
                } else {
                    Err(Error::Type {
                        expected: <$type>::NAME,
                        actual: self.type_name(),
                    })
                }
            }
        }
    };
}

impl_value_variant!(u64, Number, "number");
impl_value_variant!(String, String, "string");
impl_value_variant!(&'a Edge<'a>, Edge, "edge");
impl_value_variant!(&'a Node<'a>, Node, "node");
impl_value_variant!(Stream<'a>, Stream, "stream");
impl_value_variant!(Function<'a>, Function, "function");

impl<'dump> Function<'dump> {
    pub fn new<F>(function: F) -> Function<'dump>
    where
        F: 'dump + Callable<'dump>
    {
        Function(Rc::new(function))
    }

    /// Apply `self` to the actual parameter values `actuals`.
    ///
    /// If there are more actuals than `self` expects, pass `self` as many
    /// arguments as it does expect, and then try to apply the return value to
    /// the remaining arguments.
    ///
    /// If there are fewer actuals than `self` expects, return a `PartialApp`
    /// `Function` that retains the arguments we do have, and waits for the
    /// rest. If `actuals` is a zero-length slice, return `self` unchanged
    /// (more precisely, a `Value` of a clone of `self`).
    ///
    /// Note that arguments appear in `actuals` from left to right, and
    /// functions consume arguments from the right end: if `f` takes two
    /// arguments, `x (y (z f))` applies `f` to `&[y, z]`, and then applies
    /// whatever that call returns (it had better be a function!) to `&[z]`.
    pub fn call(&self, actuals: &[Value<'dump>], cx: &Context<'dump>)
                -> EvalResult<'dump>
    {
        let arity = self.0.arity();

        // Zero-arity functions aren't permitted, so the split below is
        // guaranteed to make progress.
        assert!(arity > 0);

        if actuals.len() < arity {
            // We don't have enough actuals to call `fun`. Create a function
            // that awaits the rest of the actuals, and then carries out the call.
            let partial = PartialApp {
                function: self.clone(),
                arity: arity - actuals.len(),
                actuals: actuals.to_owned(),
            };
            return Ok(Value::Function(Function::new(partial)));
        }

        let (unused, exact) = actuals.split_at(actuals.len() - arity);
        let value = self.0.call_exact_arity(exact, cx)?;

        if unused.is_empty() {
            return Ok(value);
        }

        // We have more arguments to pass to the result, so recur.
        if let Value::Function(next_fun) = value {
            next_fun.call(unused, cx)
        } else {
            Err(Error::NotAFunction)
        }
    }
}

struct PartialApp<'a> {
    function: Function<'a>,
    arity: usize,
    actuals: Vec<Value<'a>>,
}

impl<'dump> Callable<'dump> for PartialApp<'dump> {
    fn call_exact_arity(&self, actuals: &[Value<'dump>], cx: &Context<'dump>)
                        -> EvalResult<'dump>
    {
        let mut exact = actuals.to_owned();
        exact.extend_from_slice(&self.actuals);
        assert_eq!(exact.len(), self.function.0.arity());
        self.function.0.call_exact_arity(&exact, cx)
    }

    fn arity(&self) -> usize {
        self.arity
    }

    fn name(&self) -> Cow<str> {
        Cow::Owned(format!("partial application of {}", self.function.0.name()))
    }
}
