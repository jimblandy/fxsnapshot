use dump::{Edge, Node};

use std::io;

/// A value produced by evaluating an expression.
///
/// Values of this type may borrow contents from a `CoreDump`, hence the `'dump`
/// lifetime parameter. Furthermore, stream values also values lazily, as
/// directed by a borrowed portion of the expression, hence the `'expr` lifetime
/// parameter. Almost certainly, a `Value`
#[derive(Clone)]
pub enum Value<'a> {
    Number(u64),
    String(String),
    Edge(Edge<'a>),
    Node(Node<'a>),
    Stream(Stream<'a>)
}

pub struct Stream<'a>(Box<'a + CloneableStream<'a>>);

pub trait CloneableStream<'a> {
    fn boxed_clone(&self) -> Box<'a + CloneableStream<'a>>;
    fn next(&mut self) -> Option<Value<'a>>;
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

impl<'a> Value<'a> {
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

fn write_stream<'a>(stream: Stream<'a>, orientation: &Orientation, output: &mut io::Write)
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

impl<'a, I> CloneableStream<'a> for I
    where I: 'a + Iterator<Item=Value<'a>> + Clone
{
    fn boxed_clone(&self) -> Box<'a + CloneableStream<'a>> {
        Box::new(self.clone())
    }

    fn next(&mut self) -> Option<Value<'a>> {
        <Self as Iterator>::next(self)
    }
}

impl<'a> Stream<'a> {
    pub fn new<I>(iter: I) -> Stream<'a>
        where I: 'a + CloneableStream<'a>
    {
        Stream(Box::new(iter))
    }
}

impl<'a> Clone for Stream<'a> {
    fn clone(&self) -> Self {
        Stream(self.0.boxed_clone())
    }
}

impl<'a> Iterator for Stream<'a> {
    type Item = Value<'a>;
    fn next(&mut self) -> Option<Value<'a>> {
        self.0.next()
    }
}
