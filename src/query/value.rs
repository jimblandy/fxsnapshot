use dump::{Edge, Node};

use std::io;

#[derive(Clone)]
pub enum Value<'dump> {
    Number(u64),
    String(String),
    Edge(Edge<'dump>),
    Node(Node<'dump>),
    Stream(Stream<'dump>)
}

pub struct Stream<'dump>(Box<CloneableStream<'dump>>);

trait CloneableStream<'dump> {
    fn boxed_clone(&self) -> Box<CloneableStream<'dump>>;
    fn next(&mut self) -> Option<Value<'dump>>;
}

impl<'dump> Clone for Stream<'dump> {
    fn clone(&self) -> Self {
        Stream(self.0.boxed_clone())
    }
}

impl<'dump> Iterator for Stream<'dump> {
    type Item = Value<'dump>;
    fn next(&mut self) -> Option<Value<'dump>> {
        self.0.next()
    }
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

impl<'dump> Value<'dump> {
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

fn write_stream<'dump>(stream: Stream<'dump>,
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

