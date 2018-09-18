## fxsnapshot: query Firefox heap snapshots

**NOTE** This is still very much a sketch, barely tested, unrevised, and so on.

This program can query Firefox devtools heap snapshots, search for nodes, find
paths between nodes, and so on.

This requires Rust 1.30.0 (nightly as of 2018-9-14), for stable
Iterator::find_map.

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

### Query syntax

Things marked 'NYI' are not yet implemented.

Single values:

- number and string literals - as usual

- variable names - variables are introduced by map expressions. NYI

- `root` - the snapshot's root node

- `EXPR node` - the node whose id is `EXPR`. NYI

- `EXPR . FIELD` - the value of `FIELD` in `EXPR`, which must be a node or edge. NYI

- `EXPR referent` - the node that is the referent of the edge given by `EXPR`. NYI
  Equivalent to `EXPR .referent node`.

- `EXPR referents` - a stream of the nodes referred to by the node `EXPR`. NYI
  Equivalent to `EXPR edges map referent;`

- `STREAM first` - the first value produced by `STREAM`, or an error if `STREAM`
  is empty.

- `STREAM find PREDICATE` - the first item from `STREAM` that matches
  `PREDICATE`. Equivalent to `STREAM { PREDICATE } first`. NYI

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

- `STREAM map EXPR-TAIL ;` - Produce a new stream producing a value `V
  EXPR-TAIL` for each value `V` produced by `STREAM`. For example, `root edges
  map referent ;` is a stream of the nodes referred to by `root`'s edges. NYI

- `STREAM until PREDICATE` - the prefix of `STREAM` until the first value
  satisfying `PREDICATE`. NYI

Predicates:

-   `EXPR` - accepts values equal to the given value.

    If the expression is a string, and it's being matched against nodes or edges,
    it behaves like the predicate `name: EXPRESSION`, accepting those nodes or
    edges whose name matches the string.

-   `FIELD: PREDICATE` - a predicate on edges or nodes, accepts values whose
    given `FIELD` matches `PREDICATE`.

-   `/REGEXP/` - a predicate on strings, accepting those that match `REGEXP`.

-   `PREDICATE && PREDICATE` - the intersection of the two predicates NYI

-   `PREDICATE || PREDICATE` - the union of the two predicates

-   `! PREDICATE` - logical 'not'

-   `any PREDICATE`, `all PREDICATE` - predicates on streams. These accept if
    the stream has any elements matching `PREDICATE`, and if all the stream's
    elements match `PREDICATE`.

Predicates on paths (streams of alternating nodes and edges):

- `ends PREDICATE` - accepts the stream if its last element satisfies `PREDICATE`.

- `[ node, node, ... ]` - path whose nodes match the given predicates. NYI

- `[ node, edge:node, ... ]` - path whose nodes and edges match the given predicates. NYI

- `[ node1, ... node2, ... node3 ]` - (literal `...`) in the syntax: a path
  starting with a node that matches `node1`, passing through a node that matches
  `node2`, and ending with a node that matches `node3`. NYI

## New syntax draft

There's no reason to be too inventive.

### Haskell / SML

I'd like to try with using postfix application instead of prefix, because I'd
like to try supporting as-you-type query exploration, and typing new operators
at the end of something is easier than going to the front and inserting new
operators. `x y f` isn't that different from `f x y`, is it?

- Simple literals: numbers (`0x` hex and decimal), strings.

- identifiers: built-in functions or local variables.

- `ARG FN`: Function application. `0x10 node` applies `node` to `0x10`. This is
  right-associative, so it curries to the left: A B C D is (A (B (C D))).

- `(EXPR)`: grouping

- `.FIELD`: a function from a node or an edge to the value of the field named. Hence,
  `root.id` is an application of the function `.id` to `root`.

- `\ID. EXPR`: lambda

- `EXPR OP EXPR`: infix operator.

- `(OP EXPR)`, `(EXPR OP)`: left and right infix operator slices

and that's it. Built-in functions and constants:

- `root`: the root node.

- `nodes`: a stream of all nodes.

- `STREAM first`: the first value in `STREAM`.

- `NODE edges`: a stream of the outgoing edges of `NODE`.

- `STREAM paths` or `NODE paths`: a stream of all paths proceeding from `NODE`
  or the set of nodes given by `STREAM`.

Infix operators:

- `A = B`: equality

- `A . B`: composition

Examples:

- `nodes (\n.n.id = 0x7fc384307f80) filter`, or
- `nodes (.id . (= 0x7fc384307f80)) filter`
