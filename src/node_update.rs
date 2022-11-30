use super::stabilisation_num::StabilisationNum;
use crate::node::Incremental;
use std::cell::Cell;

pub(crate) type BoxedUpdateFn<T> = Box<dyn FnMut(NodeUpdate<&T>)>;

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
enum Previously {
    NeverBeenUpdated,
    Necessary,
    Changed,
    Invalidated,
    Unnecessary,
}

#[derive(Copy, Clone, Debug)]
pub(crate) enum NodeUpdateDelayed {
    Necessary,
    Changed,
    Invalidated,
    Unnecessary,
}

#[derive(Debug)]
pub enum NodeUpdate<T> {
    Necessary(T),
    Changed(T),
    Invalidated,
    Unnecessary,
}

pub(crate) struct OnUpdateHandler<T> {
    handler_fn: BoxedUpdateFn<T>,
    previous_update_kind: Cell<Previously>,
    created_at: StabilisationNum,
}

impl<T> OnUpdateHandler<T> {
    pub(crate) fn new(created_at: StabilisationNum, handler_fn: BoxedUpdateFn<T>) -> Self {
        OnUpdateHandler {
            handler_fn,
            created_at,
            previous_update_kind: Previously::NeverBeenUpdated.into(),
        }
    }
    fn really_run(&mut self, node: &dyn Incremental<T>, node_update: NodeUpdateDelayed) {
        self.previous_update_kind.set(match &node_update {
            NodeUpdateDelayed::Changed => Previously::Changed,
            NodeUpdateDelayed::Necessary => Previously::Necessary,
            NodeUpdateDelayed::Invalidated => Previously::Invalidated,
            NodeUpdateDelayed::Unnecessary => Previously::Unnecessary,
        });
        let value = node.value_as_ref();
        let concrete_update = match node_update {
            NodeUpdateDelayed::Changed => {
                let v = value.unwrap();
                return (self.handler_fn)(NodeUpdate::Changed(&*v));
            }
            NodeUpdateDelayed::Necessary => {
                let v = value.unwrap();
                return (self.handler_fn)(NodeUpdate::Necessary(&*v));
            }
            NodeUpdateDelayed::Invalidated => NodeUpdate::Invalidated,
            NodeUpdateDelayed::Unnecessary => NodeUpdate::Unnecessary,
        };
        (self.handler_fn)(concrete_update);
    }
    pub(crate) fn run(
        &mut self,
        node: &dyn Incremental<T>,
        node_update: NodeUpdateDelayed,
        now: StabilisationNum,
    ) {
        /* We only run the handler if was created in an earlier stabilization cycle.  If the
        handler was created by another on-update handler during the running of on-update
        handlers in the current stabilization, we treat the added handler as if it were added
        after this stabilization finished.  We will run it at the next stabilization, because
        the node with the handler was pushed on [state.handle_after_stabilization]. */
        if self.created_at < now {
            match (self.previous_update_kind.get(), node_update) {
                /* Once a node is invalidated, there will never be further information to provide,
                since incremental does not allow an invalid node to become valid. */
                (Previously::Invalidated, _) => (),
                /* These cases can happen if a node is handled after stabilization due to another
                handler.  But for the current handler, there is nothing to do because there is no
                new information to provide. */
                (Previously::Changed, NodeUpdateDelayed::Necessary)
                | (Previously::Necessary, NodeUpdateDelayed::Necessary)
                | (Previously::Unnecessary, NodeUpdateDelayed::Unnecessary) => (),
                /* If this handler hasn't seen a node that is changing, we treat the update as an
                initialization. */
                (
                    Previously::NeverBeenUpdated | Previously::Unnecessary,
                    NodeUpdateDelayed::Changed,
                ) => self.really_run(node, NodeUpdateDelayed::Necessary),
                /* All other updates are run as is. */
                (_, node_update) => self.really_run(node, node_update),
            }
        }
    }
}
