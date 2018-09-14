//! Breadth-first traversal of the graph of `Node`s in a `CoreDump`.

use dump::{CoreDump, Edge, NodeId};

use std::collections::{HashMap, VecDeque};
use std::collections::hash_map::Entry;

#[derive(Clone, Debug)]
pub struct BreadthFirst<'a> {
    /// The `CoreDump` whose nodes we are traversing.
    dump: &'a CoreDump<'a>,

    /// `Node`s we have reached so far. Each is mapped to the edge by which it
    /// was first reached, and the `Node` from which that edge originates. The
    /// nodes from which we first started the traversal are mapped to `None`.
    visited: HashMap<NodeId, Option<Step<'a>>>,

    /// The 'growth front' of nodes we have reached, but not yet produced as
    /// iteration items. Also known as 'grey nodes'. When this is empty, the
    /// traversal is over.
    front: VecDeque<NodeId>,
}

/// One step in a path: an `Edge` together with the node from which it
/// originates.
#[derive(Clone, Debug)]
pub struct Step<'a> {
    pub origin: NodeId,
    pub edge: Edge<'a>
}

impl<'a> BreadthFirst<'a> {
    pub fn new(dump: &'a CoreDump<'a>) -> BreadthFirst<'a> {
        BreadthFirst {
            dump,
            visited: HashMap::new(),
            front: VecDeque::new()
        }
    }

    /// Set the node from which traversal should begin.
    pub fn set_start_node(&mut self, node: NodeId) {
        assert!(self.dump.has_node(node));
        self.visited.insert(node, None);
        self.front.push_back(node);
    }

    /// If `id` has been reached by the traversal so far (or is a start node),
    /// return the path by which it was first reached. If `id` has not yet been
    /// reached, return `None`.
    ///
    /// The path returned is a vector of `Step`s leading from a starting node to
    /// `id`. If `id` is itself a start node, return an empty vector.
    pub fn path_from_start(&self, id: NodeId) -> Option<Vec<Step<'a>>> {
        let mut entry = match self.visited.get(&id) {
            Some(e) => e, // We have visited this node, or it is a start node.
            None => return None, // We have never encountered this node.
        };

        let mut path = Vec::new();
        while let Some(step) = entry {
            path.push(step.clone());
            // Every Step's origin should be a node we've encountered,
            // so just indexing should be okay here.
            entry = &self.visited[&step.origin];
        }
        path.reverse();
        Some(path)
    }
}

impl<'a> Iterator for BreadthFirst<'a> {
    type Item = Vec<Step<'a>>;

    /// Return a shortest path by which one can reach the next node visited in
    /// breadth-first order, or `None` if there are no more nodes reachable from
    /// the start node(s).
    ///
    /// By 'breadth-first' order, we mean:
    ///
    /// - We produce each node at most once.
    ///
    /// - The lengths of the paths only increase (or stay the same) as we
    ///   produce them; we never produce a shorter path after a longer path.
    ///
    /// - The path from any produced node back to the (or a) start node is among
    ///   the shortest such paths in the entire `CoreDump`.
    fn next(&mut self) -> Option<Vec<Step<'a>>> {
        if let Some(id) = self.front.pop_front() {
            // Look over this node's outgoing edges, and see if they reach any
            // new nodes. If so, record how we reached them, and queue them to
            // be produced later.
            let node = self.dump.get_node(id).unwrap();
            for edge in node.edges {
                if let Some(referent) = edge.referent {
                    // Have we reached this edge's referent before?
                    match self.visited.entry(referent) {
                        // We have! Ignore this edge and its referent.
                        Entry::Occupied(_) => (),

                        // No, this is the first time we've reached the
                        // referent! Record the edge by which we reached it, and
                        // queue the referent to be produced later.
                        Entry::Vacant(entry) => {
                            entry.insert(Some(Step { origin: id, edge }));
                            self.front.push_back(referent);
                        }
                    }
                }
            }

            Some(self.path_from_start(id).unwrap())
        } else {
            None
        }
    }
}
