## fxsnapshot: query Firefox heap snapshots

**NOTE** This is still very much a sketch, barely tested, unrevised, and so on.

This program can query Firefox devtools heap snapshots, search for nodes, find
paths between nodes, and so on.

This requires Rust 1.35.0 (nightly as of 2019-5-23), for stable
Iterator::find_map and Box<FnOnce>.

## Taking heap snapshots

You can make Firefox write a snapshot of its JavaScript heap to disk by
evaluating the following expression in the browser toolbox console:

    ChromeUtils.saveHeapSnapshot({ runtime: true })

The return value will be a filename ending in `.fxsnapshot` in a temporary
directory. This is a gzipped protobuf stream, of the format defined in
[CoreDump.proto][coredump] in the Firefox sources.

[coredump]: https://searchfox.org/mozilla-central/source/devtools/shared/heapsnapshot/CoreDump.proto

This program operates only on decompressed snapshots, so you'll need to
decompress it first:

    $ mv /tmp/131196117.fxsnapshot ~/today.fxsnapshot.gz
    $ gunzip today.fxsnapshot.gz

### Parent and content processes

In Firefox, all web content is held in content processes, children of the main
Firefox process. The browser console runs in the parent process, so calling
`saveHeapSnapshot` there won't get you any information about web content. If you
want a heap snapshot that contains a particular tab's memory, select that tab
and open the 'browser content toolbox' ('content' being the key word in that
noun pile), select the console tab, and call `saveHeapSnapshot` there.

## Queries

The program is invoked like this:

    $ fxsnapshot today.fxsnapshot.pb QUERY

The `QUERY` argument is an expression in a little language that operates on
snapshot edges and nodes, numbers, strings, and lazy streams of the above. For
example:

    $ fxsnapshot today.fxsnapshot.pb root
    $ fxsnapshot today.fxsnapshot.pb nodes
    $ fxsnapshot today.fxsnapshot.pb 'nodes { id: 0x7fc384307f80 }'
    $ fxsnapshot today.fxsnapshot.pb 'nodes { coarseType: "Script" }'
    $ fxsnapshot today.fxsnapshot.pb 'nodes { coarseType: "Script" } first edges'

You can find all the devtools scripts:

    $ fxsnapshot chrome.fxsnapshot 'nodes { scriptFilename: /devtools/ }'

You can use the `paths` operator to find paths with interesting characteristics,
like ending at a particular node:

    $ fxsnapshot chrome.fxsnapshot 'root paths { ends id: 0x7f412ebb2040 }'

Or to find all the closures using a given script:

    $ fxsnapshot chrome.fxsnapshot \
    > 'nodes { JSObjectClassName: "Function" } paths { ends id: 0x7f412ebb2040 } first '

### Query language

Things marked 'NYI' are not yet implemented.

The query language includes five types:

- integers, strings, booleans: as usual

- structs: a collection of named fields, like `Edge { name: "script", referent: 0x7f453e9f53a0 }`.
  Used for edges and nodes.

- streams: a stream of values of any type. Streams are lazy: values are computed only on demand.

- functions: closures that take one or more arguments.

Expression syntax:

- numeric literals, both hex (`0x12fd`) and decimal (`40`)

- string literals: `"script"`

- boolean literals: `true` and `false`

- variable names: the usual

- Function application is postfix: `x f` applies `f` to `x`. Application
  associates to the right, so `x f g` is `(x f) g`: apply `f` to `x`, and then
  apply `g` to the result.

- Function expressions: `|stream, name| stream { scriptFilename: name }` is a
  function that takes two arguments, a stream and a string, and returns a
  filtered version of the original stream.

- Predicate expressions: `{ P, ... }` is a function mapping values to booleans,
  returning true for values that match all the given predicates. Predicates have
  their own syntax, described below. Applying a predicate expression to a stream
  is an implicit filter.

- `(EXPR)`: parentheses

Predicate syntax:

- `EXPR`: matches values equal to the value of `EXPR`.

- `/REGEXP/`: matches strings that match the given regular expression. Any
  internal `/` or `\` characters must be escaped with `\` characters.

- `#/REGEXP/#`: Like `/REGEXP/`, except that no internal escapes are recognized;
  the regexp ends at the earliest `/#` sequence.

- `id: P` matches structs whose field `id` matches `P`.

- `P and Q`, `P or Q`, `not P`: conjunction, disjunction, negation

- `any P`, `all P`: matches a stream including any value matching `P`, or only
  values that match `P`.

- `ends P`: matches a stream whose last value matches `P`.

- `(P)`: parentheses

Built-in functions:

- `nodes`: Return a stream of all nodes in the heap snapshot.

- `root`: The snapshot's root node.

- `NODE edges`: Return a stream of the edges of `NODE`

- `STREAM first`: Return the first element of `STREAM`.

- `STREAM F map`: Return a stream applying `F` to each element of `STREAM`.

- `NODE paths`: Return all paths starting at `NODE`, as a stream of streams:
  `[[ NODE EDGE NODE EDGE ... NODE]]`. The paths are sorted by length, include
  only the shortest path to any given final node, and include only one path to
  any given node. If `NODE` is a stream of nodes, then produce all paths whose
  starting point is included in the stream.
