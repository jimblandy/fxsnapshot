#![allow(unused_imports, unused_variables)]

// extern crates
#[macro_use] extern crate failure;
             extern crate memmap;
             extern crate quick_protobuf;

// intra-crate modules
mod protos;

// extern crate uses
use quick_protobuf::{MessageRead, BytesReader};
use quick_protobuf::Error as QPError;

// intra-crate uses
use protos::mozilla::devtools::protobuf::{Metadata, Node, StackFrame};
use protos::mozilla::devtools::protobuf::mod_StackFrame::mod_Data::{OneOfSourceOrRef,
                                                                    OneOfFunctionDisplayNameOrRef};
use protos::mozilla::devtools::protobuf::mod_Node::{OneOfTypeNameOrRef,
                                                    OneOfJSObjectClassNameOrRef,
                                                    OneOfScriptFilenameOrRef,
                                                    OneOfdescriptiveTypeNameOrRef};
use protos::mozilla::devtools::protobuf::mod_Edge::OneOfEdgeNameOrRef;

// std uses
use failure::{Fail, Error, ResultExt};
use memmap::{Mmap, MmapOptions};
use std::collections::HashMap;
use std::borrow::Cow;
use std::fs::File;
use std::path::{Path, PathBuf};

/// A type representing a potentially de-duplicated string, either a one-byte
/// string (where `C` is `u8`) or a two-byte string (where `C` is `u16`). The
/// lifetime `'content` is the lifetime of the text of the string.
///
/// This trait should be implemented for the `BlahOrRef` `oneof` types holding
/// strings, to help build the tables mapping indices to texts.
trait StringOrRef<'content, C: 'content + Copy> {
    // Return a slice of the string's bytes, if they were serialized rather than
    // just cited by index.
    //
    // Even though this returns a &[u8], this is the right method to implement
    // for both one-byte and two-byte strings. CoreDump.proto just uses `bytes`
    // for both, so this method can be implemented harmoniously with the
    // generated parser's types. Then, the `get` default method takes care of
    // fixing up the type.
    fn get_bytes(&self) -> Option<&'content [u8]>;

    // Return a slice of the string's characters, if they were serialized rather than
    // just cited by index.
    //
    // This is the same as `get_bytes`, except that it returns a slice of the
    // appropriate element type.
    fn get(&self) -> Option<&'content [C]> {
        self.get_bytes().map(transmute_simple_slice)
    }
}

// Turn a `&[T]` into a slice of `&[U]` of the appropriate length, where both
// types are `Copy`. Panic if the length doesn't divide evenly.
fn transmute_simple_slice<T, U>(slice: &[T]) -> &[U]
    where T: Copy, U: Copy
{
    let size = std::mem::size_of::<U>();
    assert!(slice.len() % size == 0);
    unsafe {
        std::slice::from_raw_parts(slice.as_ptr() as *const U,
                                   slice.len() / size)
    }
}

macro_rules! impl_StringOrRef {
    ($enum:ident, $variant:ident, $element:ty) => {
        impl<'content> StringOrRef<'content, $element> for $enum<'content> {
            fn get_bytes(&self) -> Option<&'content [u8]> {
                match self {
                    $enum::$variant(Cow::Borrowed(r)) => Some(r),
                    $enum::$variant(Cow::Owned(_)) => panic!("unexpected owned Cow"),
                    _ => None
                }
            }
        }
    }
}

impl_StringOrRef!(OneOfSourceOrRef, source, u16);
impl_StringOrRef!(OneOfFunctionDisplayNameOrRef, functionDisplayName, u16);
impl_StringOrRef!(OneOfTypeNameOrRef, typeName, u16);
impl_StringOrRef!(OneOfJSObjectClassNameOrRef, jsObjectClassName, u8);
impl_StringOrRef!(OneOfScriptFilenameOrRef, scriptFilename, u8);
impl_StringOrRef!(OneOfdescriptiveTypeNameOrRef, descriptiveTypeName, u8);
impl_StringOrRef!(OneOfEdgeNameOrRef, name, u16);

// A snapshot node id.
#[derive(Clone, Copy, Eq, PartialEq, Hash)]
struct NodeId(u64);

// A snapshot stack frame id.
#[derive(Clone, Copy, Eq, PartialEq, Hash)]
struct FrameId(u64);

struct CoreDump<'buffer> {
    /// The filename, solely for use in error messages.
    path: PathBuf,

    /// The core dump data, mapped into memory.
    bytes: &'buffer [u8],

    /// The core dump's metadata message.
    metadata: Metadata,

    /// The root node.
    root: Node<'buffer>,

    /// A map from string indices to one-byte strings borrowed out of `bytes`.
    one_byte_strings: Vec<&'buffer [u8]>,

    /// A map from string indices to two-byte strings borrowed out of `bytes`.
    two_byte_strings: Vec<&'buffer [u16]>,

    /// A map from node id's to message offsets. To be precise, this is the
    /// offset within `bytes` of the Varint length preceding the `Node` message
    /// with the given id.
    node_offsets: HashMap<NodeId, usize>,

    /// A map from StackFrame.Data id's to message offsets. This holds the
    /// offset within `bytes` of the Varint length preceding the `Node` message
    /// with the given id.
    frame_offsets: HashMap<FrameId, usize>
}

/// A type that can intern strings of type &[C].
trait StringTable<'buffer, C: 'buffer + Copy> {
    /// Record that `string` is `self`'s next string of type `&[C]`.
    fn intern_string(&mut self, string: &'buffer [C]);

    /// Intern `oneof`'s string, if present, in `table`.
    fn intern<S>(&mut self, string_or_ref: &S)
        where S: StringOrRef<'buffer, C>
    {
        if let Some(string) = string_or_ref.get() {
            self.intern_string(string);
        }
    }

}

impl<'buffer> StringTable<'buffer, u8> for CoreDump<'buffer> {
    fn intern_string(&mut self, string: &'buffer [u8]) {
        self.one_byte_strings.push(string);
    }
}

impl<'buffer> StringTable<'buffer, u16> for CoreDump<'buffer> {
    fn intern_string(&mut self, string: &'buffer [u16]) {
        self.two_byte_strings.push(string);
    }
}

impl<'buffer> CoreDump<'buffer> {
    fn new<'p>(path: &'p Path, bytes: &'buffer [u8]) -> Result<CoreDump<'buffer>, Error> {
        let mut reader = BytesReader::from_bytes(bytes);
        let metadata: Metadata = reader.read_message(bytes)
            .context(format!("{}: couldn't read metadata:", path.display()))?;

        // Scan the entire core dump, building the map from node ids to byte
        // offsets, and the map from string numbers to strings.
        let mut dump = CoreDump {
            path: path.to_owned(),
            bytes,
            metadata,
            root: Default::default(),
            one_byte_strings: Vec::new(),
            two_byte_strings: Vec::new(),
            node_offsets: HashMap::new(),
            frame_offsets: HashMap::new(),
        };

        // Read the root node.
        let root_offset = bytes.len() - reader.len(); // ugh
        let root: Node = reader.read_message(bytes)
            .context(format!("{}: couldn't read root node:", path.display()))?;
        dump.scan_node(&root, root_offset);
        std::mem::replace(&mut dump.root, root);

        // Read all the subsequent nodes.
        while !reader.is_eof() {
            let offset = bytes.len() - reader.len(); // ugh

            // Don't format an error message unless an error actually occurs.
            let node: Node = match reader.read_message(bytes) {
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

    fn scan_node(&mut self, node: &Node<'buffer>, offset: usize) {
        use protos::mozilla::devtools::protobuf::mod_Node::*;

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

    fn scan_frame(&mut self, mut frame: &Option<StackFrame<'buffer>>, offset: usize) {
        use protos::mozilla::devtools::protobuf::mod_StackFrame::OneOfStackFrameType;
        while let Some(StackFrame { StackFrameType: OneOfStackFrameType::data(data) }) = frame {
            if let Some(id) = data.id {
                self.frame_offsets.insert(FrameId(id), offset);
            }

            self.intern(&data.SourceOrRef);
            self.intern(&data.FunctionDisplayNameOrRef);
            frame = &data.parent;
        }
    }
}

fn run() -> Result<(), Error> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();

    if args.len() != 1 {
        return Err(format_err!("Usage: fxsnapshot FILE"));
    }

    let path = Path::new(&args[0]);
    let file = File::open(path)
        .context(format!("Failed to open snapshot '{}':", path.display()))?;
    let mmap = unsafe { Mmap::map(&file)? };
    let bytes = &mmap[..];

    let dump = CoreDump::new(path, bytes)?;
    println!("metadata: {:?}", dump.metadata);

    Ok(())
}

fn main() {
    if let Err(e) = run() {
        for failure in e.iter_chain() {
            eprintln!("{}", failure);
        }
        std::process::exit(1);
    }
}
