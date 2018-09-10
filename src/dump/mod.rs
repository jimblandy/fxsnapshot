// extern crate uses
use failure::{Fail, Error, ResultExt};
use petgraph::visit;
use quick_protobuf::BytesReader;

pub mod mozilla;

// intra-crate uses
use self::mozilla::devtools::protobuf;

// std uses
use std::collections::{HashMap, HashSet};
use std::fmt::{self, Write};
use std::path::{Path, PathBuf};

pub struct CoreDump<'buffer> {
    /// The filename, solely for use in error messages.
    pub path: PathBuf,

    /// The core dump's timestamp.
    pub timestamp: Option<u64>,

    /// The core dump data, mapped into memory.
    bytes: &'buffer [u8],

    /// The offset of the root node.
    root_offset: usize,

    /// A map from deduplicated string indices to one-byte strings borrowed out
    /// of `bytes`.
    one_byte_strings: Vec<OneByteString<'buffer>>,

    /// A map from deduplicated string indices to two-byte strings borrowed out
    /// of `bytes`.
    two_byte_strings: Vec<TwoByteString<'buffer>>,

    /// A map from node id's to message offsets. To be precise, this is the
    /// offset within `bytes` of the Varint length preceding the `Node` message
    /// with the given id.
    node_offsets: HashMap<NodeId, usize>,

    /// A map from StackFrame.Data id's to message offsets. This holds the
    /// offset within `bytes` of the Varint length preceding the `Node` message
    /// with the given id.
    frame_offsets: HashMap<FrameId, usize>
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
    pub descriptiveTypeName: Option<TwoByteString<'buffer>>
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
    Other   = 0,
    Object  = 1,
    Script  = 2,
    String  = 3,
    DOMNode = 4,
}

impl<'buffer> CoreDump<'buffer> {
    pub fn new<'p>(path: &'p Path, bytes: &'buffer [u8]) -> Result<CoreDump<'buffer>, Error> {
        let mut reader = BytesReader::from_bytes(bytes);
        let metadata: protobuf::Metadata = reader.read_message(bytes)
            .context(format!("{}: couldn't read metadata:", path.display()))?;

        // Scan the entire core dump, building the maps from ids to offsets and
        // deduplicated indices to strings.

        // Create the CoreDump first, since we populate its tables in place.
        let mut dump = CoreDump {
            path: path.to_owned(),
            bytes,
            timestamp: metadata.timeStamp,
            root_offset: bytes.len() - reader.len(),
            one_byte_strings: Vec::new(),
            two_byte_strings: Vec::new(),
            node_offsets: HashMap::new(),
            frame_offsets: HashMap::new(),
        };

        // Scan all nodes and stack frames.
        while !reader.is_eof() {
            let offset = bytes.len() - reader.len(); // ugh

            // Don't format an error message unless an error actually occurs.
            let node: protobuf::Node = match reader.read_message(bytes) {
                Ok(n) => n,
                Err(e) => {
                    let msg = format!("Couldn't read node from {} at offset {:x}:",
                                      path.display(),
                                      offset);
                    return Err(e.context(msg).into());
                }
            };

            dump.scan_node(&node, offset);
        }

        Ok(dump)
    }

    pub fn node_count(&self) -> usize {
        self.node_offsets.len()
    }

    fn get_node_at_trusted_offset(&self, offset: usize) -> Node<'buffer> {
        let tail = &self.bytes[offset..];
        let mut reader = BytesReader::from_bytes(tail);
        let node: protobuf::Node = reader.read_message(tail)
            .expect("failed to read node at trusted offset");
        Node::from_protobuf(&node, self)
    }

    pub fn get_root(&self) -> Node<'buffer> {
        self.get_node_at_trusted_offset(self.root_offset)
    }

    pub fn get_node(&self, id: NodeId) -> Option<Node<'buffer>> {
        self.node_offsets.get(&id)
            .map(|&offset| {
                self.get_node_at_trusted_offset(offset)
            })
    }

    pub fn nodes<'a>(&'a self) -> impl Iterator<Item=Node<'buffer>> + Clone + 'a {
        self.node_offsets.iter()
            .map(move |(_id, &offset)| {
                self.get_node_at_trusted_offset(offset)
            })
    }
}

// Methods for scanning the protobuf stream.
//
// These methods build the map from ids to buffer offsets, and build the
// deduplicated string tables. No owning representations of nodes, edges,
// frames, strings, etc. are built; everything borrows out of the buffer.
impl<'buffer> CoreDump<'buffer> {
    fn scan_node(&mut self, node: &protobuf::Node<'buffer>, offset: usize) {
        if let Some(id) = node.id {
            self.node_offsets.insert(NodeId(id), offset);
        }
        self.intern(&node.TypeNameOrRef);
        for edge in &node.edges {
            self.intern(&edge.EdgeNameOrRef);
        }

        self.scan_frame(&node.allocationStack, offset);

        self.intern(&node.JSObjectClassNameOrRef);
        self.intern(&node.ScriptFilenameOrRef);
        self.intern(&node.descriptiveTypeNameOrRef);
    }

    fn scan_frame(&mut self, mut frame: &Option<protobuf::StackFrame<'buffer>>, offset: usize) {
        use self::mozilla::devtools::protobuf::mod_StackFrame::OneOfStackFrameType;
        while let Some(protobuf::StackFrame { StackFrameType: OneOfStackFrameType::data(data) }) = frame {
            if let Some(id) = data.id {
                self.frame_offsets.insert(FrameId(id), offset);
            }

            self.intern(&data.SourceOrRef);
            self.intern(&data.FunctionDisplayNameOrRef);
            frame = &data.parent;
        }
    }
}

/// A type that can intern strings of type C. In practice, `Self` is always
/// `CoreDump`, and `C` is `OneByteString` or `TwoByteString`.
trait StringTable<'buffer, C: 'buffer + Copy + FromDumpBytes<'buffer>> {
    /// Record that `string` is `self`'s next string of type `C`.
    fn intern_string(&mut self, string: C);

    /// Retrieve the string at index `i`.
    fn lookup(&self, i: usize) -> C;

    /// Intern `dedup`'s string, if present and given, in `table`.
    fn intern<S>(&mut self, dedup: &S)
        where S: DeduplicatedString<'buffer, C>
    {
        if let Some(Deduplicated::Given(string)) = dedup.get_deduplicated() {
            self.intern_string(string);
        }
    }

    /// Retrieve the string, if present. Consult `dump`'s back reference table
    /// if needed.
    fn get_string<D>(&self, dedup: &D) -> Option<C>
        where D: DeduplicatedString<'buffer, C>
    {
        dedup.get_deduplicated().map(|d| {
            match d {
                Deduplicated::Given(string) => string,
                Deduplicated::Ref(index) => self.lookup(index),
            }
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
    }
}

impl_StringTable!(one_byte_strings, OneByteString);
impl_StringTable!(two_byte_strings, TwoByteString);

/// A type that can be constructed from a block of bytes in a core dump.
trait FromDumpBytes<'b> {
    fn from_dump_bytes(bytes: &'b [u8]) -> Self;
}

impl<'b> FromDumpBytes<'b> for OneByteString<'b> {
    fn from_dump_bytes(bytes: &'b [u8]) -> Self {
        OneByteString(bytes)
    }
}

impl<'b> FromDumpBytes<'b> for TwoByteString<'b> {
    fn from_dump_bytes(bytes: &'b [u8]) -> Self {
        TwoByteString(transmute_simple_slice(bytes))
    }
}

// Turn a `&[T]` into a slice of `&[U]` of the appropriate length, where both
// types are `Copy`. Panic if the length doesn't divide evenly.
fn transmute_simple_slice<T, U>(slice: &[T]) -> &[U]
    where T: Copy, U: Copy
{
    let size = ::std::mem::size_of::<U>();
    assert!(slice.len() % size == 0);
    unsafe {
        ::std::slice::from_raw_parts(slice.as_ptr() as *const U,
                                     slice.len() / size)
    }
}

/// Either a value (possibly borrowing from a core dump), or an index into a
/// deduplication table.
enum Deduplicated<T> {
    Given(T),
    Ref(usize)
}

/// A type representing an optional, potentially de-duplicated string of type
/// `S` from a CoreDump file. `S` will be either `OneByteString` or
/// `TwoByteString`. The lifetime `'buffer` is the lifetime of the text of the
/// string.
///
/// This trait should be implemented for the `OneOfBlahOrRef` types that `pb-rs`
/// generates for the `CoreDump.proto` `oneof` fields holding strings.
trait DeduplicatedString<'buffer, C: 'buffer + FromDumpBytes<'buffer> + Copy> {
    /// Retrieve the string as raw bytes or a back reference index.
    fn get_bytes(&self) -> Option<Deduplicated<&'buffer [u8]>>;

    /// Retrieve the string, if present, as either a properly typed string or a
    /// back reference index.
    fn get_deduplicated(&self) -> Option<Deduplicated<C>> {
        use self::Deduplicated::*;
        self.get_bytes().map(|d| {
            match d {
                Given(bytes) => Given(C::from_dump_bytes(bytes)),
                Ref(index) => Ref(index)
            }
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
    }
}

mod string_or_ref_impls {
    use super::{Deduplicated, DeduplicatedString, OneByteString, TwoByteString};
    use super::mozilla::devtools::protobuf::mod_StackFrame::mod_Data::{OneOfSourceOrRef,
                                                                       OneOfFunctionDisplayNameOrRef};
    use super::mozilla::devtools::protobuf::mod_Node::{OneOfTypeNameOrRef,
                                                       OneOfJSObjectClassNameOrRef,
                                                       OneOfScriptFilenameOrRef,
                                                       OneOfdescriptiveTypeNameOrRef};
    use super::mozilla::devtools::protobuf::mod_Edge::OneOfEdgeNameOrRef;

    use std::borrow::Cow;

    impl_DeduplicatedString!(OneOfSourceOrRef, TwoByteString,
                             source, sourceRef);
    impl_DeduplicatedString!(OneOfFunctionDisplayNameOrRef, TwoByteString,
                             functionDisplayName, functionDisplayNameRef);
    impl_DeduplicatedString!(OneOfTypeNameOrRef, TwoByteString,
                             typeName, typeNameRef);
    impl_DeduplicatedString!(OneOfJSObjectClassNameOrRef, OneByteString,
                             jsObjectClassName, jsObjectClassNameRef);
    impl_DeduplicatedString!(OneOfScriptFilenameOrRef, OneByteString,
                             scriptFilename, scriptFilenameRef);
    impl_DeduplicatedString!(OneOfdescriptiveTypeNameOrRef, TwoByteString,
                             descriptiveTypeName, descriptiveTypeNameRef);
    impl_DeduplicatedString!(OneOfEdgeNameOrRef, TwoByteString,
                             name, nameRef);
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
            edges: proto.edges.iter()
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
        optional_field(&mut d, "size", &self.size);
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
            _ => panic!("bad coarse type value {:?}", n)
        }
    }
}

impl From<CoarseType> for String {
    fn from(c: CoarseType) -> String {
        format!("{:?}", c)
    }
}

impl<'b> visit::GraphBase for CoreDump<'b> {
    type EdgeId = ();
    type NodeId = NodeId;
}

impl<'b> visit::Visitable for CoreDump<'b> {
    type Map = HashSet<NodeId>;

    #[inline]
    fn visit_map(&self) -> Self::Map {
        HashSet::with_capacity(self.node_count())
    }

    #[inline]
    fn reset_map(&self, map: &mut Self::Map) {
        map.clear();
    }
}

pub struct NodeEdges<'buffer> {
    node: Node<'buffer>,
    i: usize
}

impl<'buffer> Iterator for NodeEdges<'buffer> {
    type Item = NodeId;

    #[inline]
    fn next(&mut self) -> Option<NodeId> {
        if self.i >= self.node.edges.len() {
            return None;
        }
        let referent = self.node.edges[self.i].referent.unwrap();
        self.i += 1;
        Some(referent)
    }
}

impl<'a, 'buffer> visit::IntoNeighbors for &'a CoreDump<'buffer> {
    type Neighbors = NodeEdges<'buffer>;

    #[inline]
    fn neighbors(self, a: NodeId) -> NodeEdges<'buffer> {
        let node = self.get_node(a)
            .expect("requested neighbors of unrecognized node id");
        NodeEdges { node, i: 0 }
    }
}

impl<'buffer> visit::NodeCount for CoreDump<'buffer> {
    #[inline]
    fn node_count(&self) -> usize {
        self.node_count()
    }
}
