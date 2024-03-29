// extern crate uses
use anyhow::{bail, Context, Error};
use quick_protobuf::BytesReader;

// Protobuf reading code generated by pb-rs.
mod generated {
    // pb-rs puts the output in a file `mozilla/mod.rs`, based on the
    // `package` declaration in `CoreDump.proto`, so let's just match
    // that. Maybe it will make it easier to add more protocols in the
    // future.
    pub mod mozilla {
        include!(concat!(env!("OUT_DIR"), "/dump/mozilla/mod.rs"));
    }
}
use self::generated::mozilla::devtools::protobuf;

// std uses
use std::collections::HashMap;
use std::fmt::{self, Write};
use std::path::{Path, PathBuf};

pub struct CoreDump<'buffer> {
    /// The filename, solely for use in error messages.
    pub path: PathBuf,

    /// The core dump's timestamp.
    pub timestamp: Option<u64>,

    /// The id of the root node.
    root_id: NodeId,

    /// A map from deduplicated string indices to one-byte strings borrowed out
    /// of the buffer holding the core dump.
    one_byte_strings: Vec<OneByteString<'buffer>>,

    /// A map from deduplicated string indices to two-byte strings borrowed out
    /// of the buffer holding the core dump.
    two_byte_strings: Vec<TwoByteString<'buffer>>,

    /// A map from node id's to parsed Nodes.
    nodes: HashMap<NodeId, Node<'buffer>>,
}

/// A ubi::Node from a core dump.
#[allow(non_snake_case)] // These names should match those found in CoreDump.proto.
#[derive(Clone)]
pub struct Node<'buffer> {
    pub id: NodeId,
    pub size: Option<Size>,
    pub edges: Vec<Edge<'buffer>>,
    // allocationStack
    pub coarseType: CoarseType,
    pub typeName: Option<TwoByteString<'buffer>>,
    pub JSObjectClassName: Option<OneByteString<'buffer>>,
    pub scriptFilename: Option<OneByteString<'buffer>>,
    pub descriptiveTypeName: Option<TwoByteString<'buffer>>,
}

/// An edge from one ubi::Node to another, from a core dump.
#[derive(Clone, Eq, PartialEq)]
pub struct Edge<'buffer> {
    pub referent: Option<NodeId>,
    pub name: Option<TwoByteString<'buffer>>,
}

/// A snapshot node id.
#[derive(Clone, Copy, Eq, PartialEq, Hash)]
pub struct NodeId(pub u64);

/// A snapshot stack frame id.
#[derive(Clone, Copy, Eq, PartialEq, Hash)]
pub struct FrameId(u64);

/// The size of an object mentioned in a core dump.
///
/// This is not `usize`, since that type represents sizes in this program's
/// memory, whereas a core dump could have been written on some other sort of
/// machine. A 32-bit `usize` might not be able to store sizes recorded on a
/// 64-bit machine.
pub type Size = u64;

/// A slice of untrusted UTF-8 text, possibly borrowed from a dump.
#[derive(Copy, Clone, Eq, PartialEq)]
pub struct OneByteString<'a>(&'a [u8]);

/// A slice of untrusted UTF-16 text, possibly borrowed from a dump.
#[derive(Copy, Clone, Eq, PartialEq)]
pub struct TwoByteString<'a>(&'a [u16]);

#[derive(Clone, Copy, Debug)]
pub enum CoarseType {
    Other = 0,
    Object = 1,
    Script = 2,
    String = 3,
    DOMNode = 4,
}

impl<'buffer> CoreDump<'buffer> {
    pub fn from_bytes<'p>(path: &'p Path, bytes: &'buffer [u8]) -> Result<CoreDump<'buffer>, Error> {
        let mut reader = BytesReader::from_bytes(bytes);
        let metadata: protobuf::Metadata = reader
            .read_message(bytes)
            .context(format!("{}: couldn't read metadata:", path.display()))?;

        // Scan the entire core dump, building the maps from ids to offsets and
        // deduplicated indices to strings.

        // Create the CoreDump first, since we populate its tables in place.
        let mut dump = CoreDump {
            path: path.to_owned(),
            timestamp: metadata.timeStamp,
            root_id: NodeId(0),
            one_byte_strings: Vec::new(),
            two_byte_strings: Vec::new(),
            nodes: HashMap::new(),
        };

        // Scan the root node.
        let root_node: protobuf::Node = reader.read_message(bytes)
            .with_context(|| format!("{}: couldn't read root node:", path.display()))?;
        dump.root_id = match root_node.id {
            None => bail!("{}: root node has no id", path.display()),
            Some(id) => NodeId(id),
        };
        dump.scan_node(&root_node);

        // Scan all remaining nodes and stack frames.
        while !reader.is_eof() {
            // Don't format an error message unless an error actually occurs.
            let node: protobuf::Node = reader.read_message(bytes)
                .with_context(|| format!(
                    "Couldn't read node from {} at offset {:x}:",
                    path.display(),
                    bytes.len() - reader.len()
                ))?;

            dump.scan_node(&node);
        }

        Ok(dump)
    }

    pub fn get_root(&self) -> &Node<'buffer> {
        // root_id had better be present in the table.
        &self.nodes[&self.root_id]
    }

    pub fn get_node(&self, id: NodeId) -> Option<&Node<'buffer>> {
        self.nodes.get(&id)
    }

    pub fn nodes<'a>(&'a self) -> impl Iterator<Item = &Node<'buffer>> + Clone + 'a {
        self.nodes.values()
    }

    pub fn has_node(&self, id: NodeId) -> bool {
        self.nodes.contains_key(&id)
    }
}

// Methods for scanning the protobuf stream.
//
// These methods build the map from ids to buffer offsets, and build the
// deduplicated string tables. No owning representations of nodes, edges,
// frames, strings, etc. are built; everything borrows out of the buffer.
impl<'buffer> CoreDump<'buffer> {
    fn scan_node(&mut self, proto: &protobuf::Node<'buffer>) {
        self.intern(&proto.TypeNameOrRef);
        for edge in &proto.edges {
            self.intern(&edge.EdgeNameOrRef);
        }

        self.scan_frame(proto.allocationStack.as_ref());

        self.intern(&proto.JSObjectClassNameOrRef);
        self.intern(&proto.ScriptFilenameOrRef);
        self.intern(&proto.descriptiveTypeNameOrRef);

        let node = Node::from_protobuf(proto, self);
        self.nodes.insert(node.id, node);
    }

    fn scan_frame(&mut self, mut frame: Option<&protobuf::StackFrame<'buffer>>) {
        use self::generated::mozilla::devtools::protobuf::mod_StackFrame::OneOfStackFrameType;
        while let Some(protobuf::StackFrame {
            StackFrameType: OneOfStackFrameType::data(data),
        }) = frame
        {
            self.intern(&data.SourceOrRef);
            self.intern(&data.FunctionDisplayNameOrRef);
            frame = data.parent.as_deref();
        }
    }
}

impl<'a> fmt::Debug for CoreDump<'a> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(fmt, "CoreDump {{ path: {:?} }}", self.path.display())
    }
}

/// A type that can intern strings of type C. In practice, `Self` is always
/// `CoreDump`, and `C` is `OneByteString` or `TwoByteString`.
trait StringTable<'buffer, C: 'buffer + Copy + From<&'buffer [u8]>> {
    /// Record that `string` is `self`'s next string of type `C`.
    fn intern_string(&mut self, string: C);

    /// Retrieve the `C` string at index `i`.
    fn lookup(&self, i: usize) -> C;

    /// Intern `dedup`'s string, if present and given, in `self`.
    fn intern<S>(&mut self, dedup: &S)
    where
        S: DeduplicatedString<'buffer, C>,
    {
        if let Some(Deduplicated::Given(string)) = dedup.get_deduplicated() {
            self.intern_string(string);
        }
    }

    /// Retrieve the string, if present. Consult `self`'s back reference table
    /// if needed.
    fn get_string<D>(&self, dedup: &D) -> Option<C>
    where
        D: DeduplicatedString<'buffer, C>,
    {
        dedup.get_deduplicated().map(|d| match d {
            Deduplicated::Given(string) => string,
            Deduplicated::Ref(index) => self.lookup(index),
        })
    }
}

macro_rules! impl_StringTable {
    ($table:ident, $type:ident) => {
        impl<'b> StringTable<'b, $type<'b>> for CoreDump<'b> {
            fn intern_string(&mut self, string: $type<'b>) {
                self.$table.push(string);
            }

            fn lookup(&self, i: usize) -> $type<'b> {
                self.$table[i]
            }
        }
    };
}

impl_StringTable!(one_byte_strings, OneByteString);
impl_StringTable!(two_byte_strings, TwoByteString);

impl<'b> From<&'b [u8]> for OneByteString<'b> {
    fn from(bytes: &'b [u8]) -> Self {
        OneByteString(bytes)
    }
}

impl<'b> From<&'b [u8]> for TwoByteString<'b> {
    fn from(bytes: &'b [u8]) -> Self {
        TwoByteString(transmute_simple_slice(bytes))
    }
}

// Turn a `&[T]` into a slice of `&[U]` of the appropriate length, where both
// types are `Copy`. Panic if the length doesn't divide evenly.
fn transmute_simple_slice<T, U>(slice: &[T]) -> &[U]
where
    T: Copy,
    U: Copy,
{
    let size = ::std::mem::size_of::<U>();
    assert!(slice.len() % size == 0);
    unsafe { ::std::slice::from_raw_parts(slice.as_ptr() as *const U, slice.len() / size) }
}

/// Either a value (possibly borrowing from a core dump), or an index into a
/// deduplication table.
enum Deduplicated<T> {
    Given(T),
    Ref(usize),
}

/// A type representing an optional, potentially de-duplicated string of type
/// `S` from a CoreDump file. `S` will be either `OneByteString` or
/// `TwoByteString`. The lifetime `'buffer` is the lifetime of the text of the
/// string.
///
/// This trait should be implemented for the `OneOfBlahOrRef` types that `pb-rs`
/// generates for the `CoreDump.proto` `oneof` fields holding strings.
trait DeduplicatedString<'buffer, C: 'buffer + From<&'buffer [u8]> + Copy> {
    /// Retrieve the string as raw bytes or a back reference index.
    fn get_bytes(&self) -> Option<Deduplicated<&'buffer [u8]>>;

    /// Retrieve the string, if present, as either a properly typed string or a
    /// back reference index.
    fn get_deduplicated(&self) -> Option<Deduplicated<C>> {
        use self::Deduplicated::*;
        self.get_bytes().map(|d| match d {
            Given(bytes) => Given(C::from(bytes)),
            Ref(index) => Ref(index),
        })
    }
}

macro_rules! impl_DeduplicatedString {
    ($enum:ident, $string:tt, $given:ident, $backref:ident) => {
        impl<'buffer> DeduplicatedString<'buffer, $string<'buffer>> for $enum<'buffer> {
            fn get_bytes(&self) -> Option<Deduplicated<&'buffer [u8]>> {
                match self {
                    $enum::$given(Cow::Borrowed(r)) => Some(Deduplicated::Given(r)),
                    $enum::$given(Cow::Owned(_)) => panic!("unexpected owned string"),
                    $enum::$backref(index) => Some(Deduplicated::Ref(*index as usize)),
                    $enum::None => None,
                }
            }
        }
    };
}

mod string_or_ref_impls {
    use super::generated::mozilla::devtools::protobuf::mod_Edge::OneOfEdgeNameOrRef;
    use super::generated::mozilla::devtools::protobuf::mod_Node::{
        OneOfJSObjectClassNameOrRef, OneOfScriptFilenameOrRef, OneOfTypeNameOrRef,
        OneOfdescriptiveTypeNameOrRef,
    };
    use super::generated::mozilla::devtools::protobuf::mod_StackFrame::mod_Data::{
        OneOfFunctionDisplayNameOrRef, OneOfSourceOrRef,
    };
    use super::{Deduplicated, DeduplicatedString, OneByteString, TwoByteString};

    use std::borrow::Cow;

    impl_DeduplicatedString!(OneOfSourceOrRef, TwoByteString, source, sourceRef);
    impl_DeduplicatedString!(
        OneOfFunctionDisplayNameOrRef,
        TwoByteString,
        functionDisplayName,
        functionDisplayNameRef
    );
    impl_DeduplicatedString!(OneOfTypeNameOrRef, TwoByteString, typeName, typeNameRef);
    impl_DeduplicatedString!(
        OneOfJSObjectClassNameOrRef,
        OneByteString,
        jsObjectClassName,
        jsObjectClassNameRef
    );
    impl_DeduplicatedString!(
        OneOfScriptFilenameOrRef,
        OneByteString,
        scriptFilename,
        scriptFilenameRef
    );
    impl_DeduplicatedString!(
        OneOfdescriptiveTypeNameOrRef,
        TwoByteString,
        descriptiveTypeName,
        descriptiveTypeNameRef
    );
    impl_DeduplicatedString!(OneOfEdgeNameOrRef, TwoByteString, name, nameRef);
}

impl<'a> fmt::Display for OneByteString<'a> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        // This will only allocate when the string contains ill-formed UTF-8.
        // Hopefully, this will be rare enough that the allocations won't affect
        // performance.
        fmt::Display::fmt(&String::from_utf8_lossy(self.0), fmt)
    }
}

impl<'a> fmt::Debug for OneByteString<'a> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        let mut buf = String::new();
        buf.push_str("b\"");
        for &byte in self.0 {
            if byte.is_ascii() && !byte.is_ascii_control() {
                buf.push(byte as char);
            } else {
                write!(&mut buf, "\\x{:02x}", byte).unwrap();
            }
        }
        buf.push('"');
        fmt.write_str(&buf)
    }
}

impl<'a> fmt::Display for TwoByteString<'a> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        let mut buf = String::new();
        for unit in ::std::char::decode_utf16(self.0.iter().cloned()) {
            match unit {
                Ok(ch) => buf.push(ch),
                Err(_) => buf.push(::std::char::REPLACEMENT_CHARACTER),
            }
        }
        fmt.write_str(&buf)
    }
}

impl<'a> fmt::Debug for TwoByteString<'a> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        fmt.write_char('"')?;
        for unit in ::std::char::decode_utf16(self.0.iter().cloned()) {
            match unit {
                Ok(ch) => fmt.write_char(ch)?,
                Err(e) => write!(fmt, "\\u{{{:04x}}}", e.unpaired_surrogate())?,
            }
        }
        fmt.write_char('"')?;
        Ok(())
    }
}

impl<'b> Node<'b> {
    fn from_protobuf(proto: &protobuf::Node<'b>, dump: &CoreDump<'b>) -> Node<'b> {
        Node {
            id: NodeId(proto.id.unwrap()),
            size: proto.size,
            edges: proto
                .edges
                .iter()
                .map(|pe| Edge::from_protobuf(pe, dump))
                .collect(),
            coarseType: CoarseType::from(proto.coarseType),
            typeName: dump.get_string(&proto.TypeNameOrRef),
            JSObjectClassName: dump.get_string(&proto.JSObjectClassNameOrRef),
            scriptFilename: dump.get_string(&proto.ScriptFilenameOrRef),
            descriptiveTypeName: dump.get_string(&proto.descriptiveTypeNameOrRef),
        }
    }
}

impl<'b> Default for Node<'b> {
    fn default() -> Node<'b> {
        Node {
            id: NodeId(0),
            size: None,
            edges: Vec::new(),
            coarseType: CoarseType::Other,
            typeName: None,
            JSObjectClassName: None,
            scriptFilename: None,
            descriptiveTypeName: None,
        }
    }
}

fn optional_field<T: fmt::Debug>(dbg: &mut fmt::DebugStruct, label: &str, opt: &Option<T>) {
    if let Some(t) = opt {
        dbg.field(label, t);
    }
}

impl<'b> fmt::Debug for Node<'b> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        let mut d = fmt.debug_struct("Node");
        d.field("id", &self.id);
        d.field("coarseType", &self.coarseType);
        optional_field(&mut d, "typeName", &self.typeName);
        optional_field(&mut d, "size", &self.size);
        optional_field(&mut d, "JSObjectClassName", &self.JSObjectClassName);
        optional_field(&mut d, "scriptFilename", &self.scriptFilename);
        optional_field(&mut d, "descriptiveTypeName", &self.descriptiveTypeName);
        d.finish()
    }
}

impl<'b> Edge<'b> {
    fn from_protobuf(proto: &protobuf::Edge<'b>, dump: &CoreDump<'b>) -> Edge<'b> {
        Edge {
            referent: proto.referent.map(NodeId),
            name: dump.get_string(&proto.EdgeNameOrRef),
        }
    }
}

impl<'b> fmt::Debug for Edge<'b> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        let mut d = fmt.debug_struct("Edge");
        optional_field(&mut d, "name", &self.name);
        optional_field(&mut d, "referent", &self.referent);
        d.finish()
    }
}

impl fmt::Debug for NodeId {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(fmt, "0x{:x}", self.0)
    }
}

impl From<u32> for CoarseType {
    fn from(n: u32) -> CoarseType {
        match n {
            0 => CoarseType::Other,
            1 => CoarseType::Object,
            2 => CoarseType::Script,
            3 => CoarseType::String,
            4 => CoarseType::DOMNode,
            _ => panic!("bad coarse type value {:?}", n),
        }
    }
}

impl From<CoarseType> for String {
    fn from(c: CoarseType) -> String {
        format!("{:?}", c)
    }
}
