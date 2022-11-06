use crate::{v_rch::expert::*, WeakIncr};
use super::*;

pub(crate) fn create<T, C, F, O>(
    state: &Rc<State>,
    recompute: F, 
    on_observability_change: O,
) -> Incr<T>
where
    T: Value,
    C: Value,
    F: FnMut() -> T + 'static,
    O: FnMut(bool) + 'static,
{
    let rc = Rc::new(ExpertNode::new_obs(recompute, on_observability_change));
    let node = Node::<ExpertNode<T, C, F, O>>::create_rc(
        state.weak(),
        state.current_scope(),
        Kind::Expert(rc),
    );

    // if debug
    // then
    //     if Option.is_some state.only_in_debug.currently_running_node
    //     then
    //         state.only_in_debug.expert_nodes_created_by_current_node
    //             <- T node :: state.only_in_debug.expert_nodes_created_by_current_node;

    Incr { node }
}

pub(crate) fn create_cyclic<T, C, Cyclic, F, O>(
    state: &Rc<State>,
    cyclic: Cyclic,
    on_observability_change: O,
) -> Incr<T>
where
    T: Value,
    C: Value,
    Cyclic: FnOnce(WeakIncr<T>) -> F,
    F: FnMut() -> T + 'static,
    O: FnMut(bool) + 'static,
{
    let node = Rc::<Node<_>>::new_cyclic(|weak| {
        let weak_incr = WeakIncr(weak.clone());
        let recompute = cyclic(weak_incr);
        let rc = Rc::new(ExpertNode::new_obs(recompute, on_observability_change));
        let mut node = Node::<ExpertNode<T, C, F, O>>::create(
            state.weak(),
            state.current_scope(),
            Kind::Expert(rc),
        );
        node.weak_self = weak.clone();
        node
    });
    node.created_in.add_node(node.clone());
    Incr{node}
    
}

pub(crate) fn make_stale(node: &NodeRef) {
    node.expert_make_stale();
}

pub(crate) fn invalidate(node: &NodeRef) {
    let state = node.state();
    #[cfg(debug_assertions)]
    node.assert_currently_running_node_is_child("invalidate");
    node.invalidate_node();
    state.propagate_invalidity();
}

pub(crate) fn add_dependency(node: &NodeRef, edge: PackedEdge) {
    node.expert_add_dependency(edge);
}

pub(crate) fn remove_dependency(node: &NodeRef, edge: PackedEdge) {
    node.expert_remove_dependency(edge);
}
