
impl FieldPredicate {
    fn is_id(&self) -> bool {
        match self {
            FieldPredicate::Id(_) => true,
        }
    }

    fn matches(&self, node: &Node) -> bool {
        match self {
            &FieldPredicate::Id(id) => { node.id.map(NodeId) == Some(id) }
        }
    }
}

impl Query {
    pub(super) fn run<K>(&self, dump: &CoreDump, cont: &mut K)
        where K: FnMut(&Node)
    {
        match self {
            Query::Node(preds) => self.node_query(dump, preds, cont),
            Query::Root => cont(&dump.root),
        }
    }

    fn node_query<K>(&self, dump: &CoreDump, preds: &[FieldPredicate], cont: &mut K)
        where K: FnMut(&Node)
    {
        // If the list includes an `id` predicate, then there's no need to
        // iterate over all nodes.
        if let Some(&FieldPredicate::Id(id)) = preds.iter().find(|f| f.is_id()) {
            if let Some(node) = dump.get_node(id) {
                if preds.iter().all(|p| p.matches(&node)) {
                    cont(&node);
                }
            }

            return;
        }

        // We need to iterate over all nodes.
        for (_id, &offset) in &dump.node_offsets {
            let node = dump.get_node_at_offset(offset).unwrap();
            if preds.iter().all(|p| p.matches(&node)) {
                cont(&node);
            }
        }
    }
}
