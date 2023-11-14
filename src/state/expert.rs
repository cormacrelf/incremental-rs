use super::*;
use crate::kind;
use crate::node::Incremental;
use crate::WeakIncr;
use kind::expert::{IsEdge, PackedEdge};

pub(crate) fn create<T, F, O>(state: &State, recompute: F, on_observability_change: O) -> Incr<T>
where
    T: Value,
    F: FnMut() -> T + 'static,
    O: FnMut(bool) + 'static,
{
    let node = Node::<kind::ExpertNode<T, F, O>>::create_rc(
        state.weak(),
        state.current_scope(),
        Kind::Expert(kind::ExpertNode::new_obs(
            recompute,
            on_observability_change,
        )),
    );

    // if debug
    // then
    //     if Option.is_some state.only_in_debug.currently_running_node
    //     then
    //         state.only_in_debug.expert_nodes_created_by_current_node
    //             <- T node :: state.only_in_debug.expert_nodes_created_by_current_node;

    Incr { node }
}

pub(crate) fn create_cyclic<T, Cyclic, F, O>(
    state: &State,
    cyclic: Cyclic,
    on_observability_change: O,
) -> Incr<T>
where
    T: Value,
    Cyclic: FnOnce(WeakIncr<T>) -> F,
    F: FnMut() -> T + 'static,
    O: FnMut(bool) + 'static,
{
    let node = Rc::<Node<_>>::new_cyclic(|weak| {
        let weak_incr = WeakIncr(weak.clone());
        let recompute = cyclic(weak_incr);
        let mut node = Node::<kind::ExpertNode<T, F, O>>::create(
            state.weak(),
            state.current_scope(),
            Kind::Expert(kind::ExpertNode::new_obs(
                recompute,
                on_observability_change,
            )),
        );
        node.weak_self = weak.clone();
        node
    });
    node.created_in.add_node(node.clone());
    Incr { node }
}

pub(crate) fn make_stale(node: &NodeRef) {
    node.expert_make_stale();
}

pub(crate) fn invalidate(node: &NodeRef) {
    let state = node.state();
    #[cfg(debug_assertions)]
    node.assert_currently_running_node_is_child("invalidate");
    node.invalidate_node(&state);
    state.propagate_invalidity();
}

pub(crate) fn add_dependency(node: &NodeRef, edge: PackedEdge) {
    node.expert_add_dependency(edge);
}

pub(crate) fn remove_dependency<T>(node: &dyn Incremental<T>, dyn_edge: &dyn IsEdge) {
    node.expert_remove_dependency(dyn_edge);
}
