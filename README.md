## fxsnapshot: query Firefox heap snapshots

This program can query Firefox devtools heap snapshots, search for nodes, find
paths between nodes, and so on.

## Queries

The program is invoked like this:

    $ fxsnapshot today.fxsnapshot.pb QUERY

Relations:

    Edge { out: Id, in: Id, name: String }
    Node {
       id: Id,
       typename: String,
       size: Num,
       stack: Frame,
       jsObjectClassName: String,
       coarseType: CoarseType,
       scriptFilename: String,
       descriptiveTypeName: String
    }

Derived:

    reachable: relation(Id, Id).
    reachable(start, start, []).
    reachable(start, end, excluded) :- Edge { from: start, to: next }, reachable(next, end).

    allocated_at(id, stack) :- node { id, stack }.

Plotting trees:

    .tree root, child_relation







The `QUERY` argument is an expression in a little language that operates on snapshot edges and nodes. It also includes numbers, strings, various enumerated types, and lazy streams of all the above. For example:

    node { coarseType: Script }

evaluates to a stream of all nodes whose `coarseType` field is `Script` - that
is, all JSScripts. Or:

    node \n.n edges



Single values:

- number and string literals - as usual

- variable names - variables are introduced by map expressions.

- `root` - the snapshot's root node

- `first STREAM` - the first item produced by `STREAM`, or an error if `STREAM`
  is empty.

- `STREAM find PREDICATE` - the first item from `STREAM` that matches
  `PREDICATE`. Equivalent to `first STREAM { PREDICATE }`

Streams:

- `[ EXPR, ... ]` - a fixed-length stream consisting of the given values.

- `nodes` - a stream of all nodes, in a random order.

- `edges EXPR` - a stream of the edges of the node that `EXPR` evaluates to.

- `STREAM { PREDICATE }` - filter `STREAM` by `PREDICATE`. In some cases the
  evaluator optimizes the evaluation of `STREAM` to produce only values matching
  `PREDICATE`.

- `STREAM \ VAR . EXPR` - maps the function `lambda VAR . EXPR` over the values
  of `STREAM`.

- `STREAM until PREDICATE` - the prefix of `STREAM` until the first value
  satisfying `PREDICATE`.

- `paths STREAM` - given `STREAM`, a finite stream of ids, generate a stream of
  all paths that begin with any id from `STREAM`. Here, a 'path' is a non-empty
  stream of edges. Shortest paths are generated first. The paths contain no loops.

Predicates:

-   `EXPR` - accepts values equal to the given value.

    If the expression is a string, and it's being matched against nodes or edges,
    it behaves like the predicate `name: EXPRESSION`, accepting those nodes or
    edges whose name matches the string.

-   `FIELD: PREDICATE` - a predicate on edges or nodes, true when the value of the
    given `FIELD` matches `PREDICATE`.

-   `PREDICATE , PREDICATE` - the intersection of the two predicates

-   `PREDICATE || PREDICATE` - the union of the two predicates

-   `! PREDICATE` - logical 'not'

-   `/REGEXP/` - a predicate on strings, matching those that match `REGEXP`.

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

    $ mv /tmp/131196117.fxsnapshot ~/today.fxsnapshot
    $ gunzip < today.fxsnapshot > today.fxsnapshot.pb

### Next steps

A patch to make `fxsnapshot` operate directly on a compressed file would be
welcome. However, decompressing a 13MiB snapshot takes about 0.3s, so it's
probably nice to retain the ability to operate on uncompressed files by just
mapping them into memory.

### TODO

- specify our own trait for formatting values?
- Use From<&[u8]> instead of FromDumpBytes?
- static type checking
