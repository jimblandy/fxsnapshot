use dump::{Edge, Node};
use failure;

use fallible_iterator::FallibleIterator;

use std::cmp::PartialEq;
use std::io;
use std::rc::Rc;

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

    // This `Rc` and the `Box` in the `Stream` struct are both necessary, and
    // each serves different purposes.
    //
    // A stream value in a query must represent the same sequence of values each
    // time it's operated on, but consuming values from a stream is a
    // destructive operation, so a stream must be cloned before we can draw
    // values from it. An `Rc` holds its contents immutable, forcing us to do
    // this clone, while also bounding the cost of cloning a `Value`.
    //
    // The `Box` forms the trait objects necessary for us to build trees of
    // adapters (filter, map) dynamically, as directed by the query expression.
    // Unlike `Rc`, `Box` implements `DerefMut`, so it's suitable for use in the
    // midst of the iterator structure, since `Iterator::next` requires `&mut
    // self`. And `Box`'s `Clone` is a deep copy, as required for preparing the
    // iterators for use.
    Stream(Rc<Stream<'a>>),
}

/// The result of evaluating an expression: either a value, or a
/// [`value::Error`](#type.Error).
pub type EvalResult<'a> = Result<Value<'a>, Error>;

pub struct Stream<'a>(Box<'a + CloneableStream<'a>>);

pub trait CloneableStream<'a> {
    fn boxed_clone(&self) -> Box<'a + CloneableStream<'a>>;
    fn cs_next(&mut self) -> Result<Option<Value<'a>>, Error>;
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
    pub fn top_write(&self, stream: &mut io::Write) -> Result<(), failure::Error> {
        self.write(&Orientation::Vertical(0), stream)
    }

    /// Write `self` to `stream`. If it is a stream, lay it out using `orientation`.
    fn write(
        &self,
        orientation: &Orientation,
        stream: &mut io::Write,
    ) -> Result<(), failure::Error> {
        match self {
            Value::Number(n) => write!(stream, "{}", n)?,
            Value::String(s) => write!(stream, "{}", s)?,
            Value::Edge(e) => write!(stream, "{:?}", e)?,
            Value::Node(n) => write!(stream, "{:?}", n)?,
            Value::Stream(s) => {
                return write_stream(Stream::unshare_ref(s), orientation, stream);
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
        }
    }
}

fn write_stream<'a>(
    mut stream: Stream<'a>,
    orientation: &Orientation,
    output: &mut io::Write,
) -> Result<(), failure::Error> {
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
impl_value_variant!(Rc<Stream<'a>>, Stream, "stream");

impl<'a, I> CloneableStream<'a> for I
where
    I: 'a + FallibleIterator<Item = Value<'a>, Error = Error> + Clone,
{
    fn boxed_clone(&self) -> Box<'a + CloneableStream<'a>> {
        Box::new(self.clone())
    }

    fn cs_next(&mut self) -> Result<Option<Value<'a>>, Error> {
        <Self as FallibleIterator>::next(self)
    }
}

impl<'a> Stream<'a> {
    pub fn new<I>(iter: I) -> Stream<'a>
    where
        I: 'a + CloneableStream<'a>,
    {
        Stream(Box::new(iter))
    }

    pub fn unshare(rc: Rc<Stream<'a>>) -> Stream<'a> {
        Rc::try_unwrap(rc).unwrap_or_else(|rc| (*rc).clone())
    }

    pub fn unshare_ref(rc: &Rc<Stream<'a>>) -> Stream<'a> {
        (**rc).clone()
    }
}

impl<'a> Clone for Stream<'a> {
    fn clone(&self) -> Self {
        Stream(self.0.boxed_clone())
    }
}

impl<'a> FallibleIterator for Stream<'a> {
    type Item = Value<'a>;
    type Error = Error;
    fn next(&mut self) -> Result<Option<Value<'a>>, Error> {
        self.0.cs_next()
    }
}
