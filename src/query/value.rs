use dump::{Edge, Node};

use std::io;

/// A value produced by evaluating an expression.
///
/// Values of this type may borrow contents from a `CoreDump`, hence the `'dump`
/// lifetime parameter. Furthermore, stream values also values lazily, as
/// directed by a borrowed portion of the expression, hence the `'expr` lifetime
/// parameter. Almost certainly, a `Value`
#[derive(Clone)]
pub enum Value<'expr, 'dump: 'expr> {
    Number(u64),
    String(String),
    Edge(Edge<'dump>),
    Node(Node<'dump>),
    Stream(Stream<'expr, 'dump>)
}

pub struct Stream<'expr, 'dump: 'expr>(Box<'expr + CloneableStream<'expr, 'dump>>);

pub trait CloneableStream<'expr, 'dump> {
    fn boxed_clone(&self) -> Box<'expr + CloneableStream<'expr, 'dump>>;
    fn next(&mut self) -> Option<Value<'expr, 'dump>>;
}

/// How to lay out elements of a stream when printed: one per line, or
/// space-separated fields on one line.
enum Orientation {
    /// The sequence is laid out on a single line, with values separated by
    /// spaces. The `usize` indicates the depth of indentation of the entire line.
    Horizontal(usize),

    /// A sequence laid out as a series of lines. The `usize` indicates the
    /// depth of indentation applied to each element.
    Vertical(usize)
}

impl<'expr, 'dump: 'expr> Value<'expr, 'dump> {
    pub fn top_write(&self, stream: &mut io::Write) -> io::Result<()> {
        self.write(&Orientation::Vertical(0), stream)
    }

    /// Write `self` to `stream`. If it is a stream, lay it out using `orientation`.
    fn write(&self, orientation: &Orientation, stream: &mut io::Write) -> io::Result<()> {
        match self {
            Value::Number(n) => write!(stream, "{}", n),
            Value::String(s) => write!(stream, "{}", s),
            Value::Edge(e) => write!(stream, "{:?}", e),
            Value::Node(n) => write!(stream, "{:?}", n),
            Value::Stream(s) => write_stream(s.clone(), orientation, stream),
        }
    }
}

fn write_stream<'expr, 'dump: 'expr>(stream: Stream<'expr, 'dump>,
                                     orientation: &Orientation,
                                     output: &mut io::Write)
                                     -> io::Result<()>
{
    match orientation {
        Orientation::Horizontal(indent) => {
            // Any streams nested within this one should be laid out vertically,
            // and indented relative to us.
            let nested_orientation = Orientation::Vertical(indent + 4);
            write!(output, "[ ")?;
            let mut first = true;
            for value in stream {
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

            write!(output, "[\n")?;
            for value in stream {
                write!(output, "{:1$}", "", indent)?;
                value.write(&nested_orientation, output)?;
                write!(output, "\n")?;
            }
            write!(output, "{:1$}]", "", indent)?;
        }
    }
    Ok(())
}

impl<'expr, 'dump, I> CloneableStream<'expr, 'dump> for I
    where 'dump: 'expr,
          I: 'expr + Iterator<Item=Value<'expr, 'dump>> + Clone
{
    fn boxed_clone(&self) -> Box<'expr + CloneableStream<'expr, 'dump>> {
        Box::new(self.clone())
    }

    fn next(&mut self) -> Option<Value<'expr, 'dump>> {
        <Self as Iterator>::next(self)
    }
}

impl<'expr, 'dump: 'expr> Stream<'expr, 'dump> {
    pub fn new<I>(iter: I) -> Stream<'expr, 'dump>
        where I: 'expr + CloneableStream<'expr, 'dump>
    {
        Stream(Box::new(iter))
    }
}

impl<'expr, 'dump: 'expr> Clone for Stream<'expr, 'dump> {
    fn clone(&self) -> Self {
        Stream(self.0.boxed_clone())
    }
}

impl<'expr, 'dump: 'expr> Iterator for Stream<'expr, 'dump> {
    type Item = Value<'expr, 'dump>;
    fn next(&mut self) -> Option<Value<'expr, 'dump>> {
        self.0.next()
    }
}
