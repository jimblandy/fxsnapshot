## fxsnapshot: query Firefox heap snapshots

**NOTE** This is still very much a sketch, barely tested, unrevised, and so on.

This program can query Firefox devtools heap snapshots, search for nodes, find
paths between nodes, and so on.

This requires Rust 1.30.0 (nightly as of 2018-9-14), for stable
Iterator::find_map.

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

    $ fxsnapshot chrome.fxsnapshot 'root paths ends id: 0x7f412ebb2040'

Or to find all the closures using a given script:

    $ fxsnapshot chrome.fxsnapshot \
    > 'nodes { JSObjectClassName: "Function" } paths ends id: 0x7f412ebb2040 first'

Most of the below is still unimplemented, and may not be coherent, but here's
the general plan:

Single values:

- number and string literals - as usual

- variable names - variables are introduced by map expressions.

- `root` - the snapshot's root node

- `STREAM first` - the first value produced by `STREAM`, or an error if `STREAM`
  is empty.

- `STREAM find PREDICATE` - the first item from `STREAM` that matches
  `PREDICATE`. Equivalent to `STREAM { PREDICATE } first`

Streams:

- `[ EXPR, ... ]` - a fixed-length stream consisting of the given values.

- `nodes` - a stream of all nodes, in a random order.

- `EXPR edges` - a stream of the edges of the node that `EXPR` evaluates to.

- `STREAM { PREDICATE }` - filter `STREAM` by `PREDICATE`. In some cases the
  evaluator optimizes the evaluation of `STREAM` to produce only values matching
  `PREDICATE`.

- `STREAM paths` - given `STREAM`, a finite stream of ids, generate a stream of
  all paths that begin with any id from `STREAM`. Here, a 'path' is a non-empty
  stream of edges. Shortest paths are generated first. The paths contain no loops.

- `STREAM \ VAR . EXPR` - maps the function `lambda VAR . EXPR` over the values
  of `STREAM`.

- `STREAM until PREDICATE` - the prefix of `STREAM` until the first value
  satisfying `PREDICATE`.

Predicates:

-   `EXPR` - accepts values equal to the given value.

    If the expression is a string, and it's being matched against nodes or edges,
    it behaves like the predicate `name: EXPRESSION`, accepting those nodes or
    edges whose name matches the string.

-   `FIELD: PREDICATE` - a predicate on edges or nodes, accepts values whose
    given `FIELD` matches `PREDICATE`.

-   `ends PREDICATE` - a predicate on streams, accepts the stream if its last
    element satisfies `PREDICATE`.

-   `/REGEXP/` - a predicate on strings, accepting those that match `REGEXP`.

-   `PREDICATE , PREDICATE` - the intersection of the two predicates

-   `PREDICATE || PREDICATE` - the union of the two predicates

-   `! PREDICATE` - logical 'not'

Predicates on edges:

- string in double quotes - an edge with the given name

Predicates on paths:

- `[ node, node, ... ]` - path whose nodes match the given predicates.

- `[ node, edge:node, ... ]` - path whose nodes and edges match the given predicates.

- `[ node, ... node, ... node ]` - (literal `...`) in the syntax: a path starting 

- `format template`

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

