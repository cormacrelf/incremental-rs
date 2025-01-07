use crate::node::ErasedNode;
use crate::CellIncrement;
use crate::NodeRef;
use std::cell::{Cell, Ref, RefCell};
use std::collections::VecDeque;
use std::fmt;

type Queue = RefCell<VecDeque<NodeRef>>;

pub(crate) struct RecomputeHeap {
    queues: RefCell<Vec<Queue>>,
    height_lower_bound: Cell<i32>,
    length: Cell<usize>,
    swap: Queue,
}

impl fmt::Debug for RecomputeHeap {
    #[rustfmt::skip::macros]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "RecomputeHeap {{")?;
        writeln!(f, "  length: {},", self.len())?;
        writeln!(
            f,
            "  height_lower_bound: {},",
            self.height_lower_bound.get()
        )?;
        writeln!(f, "  ---")?;
        let mut skip = 0;
        for (ix, q) in self.queues.borrow().iter().map(RefCell::borrow).enumerate() {
            if q.is_empty() {
                skip += 1;
                continue;
            }
            if skip != 0 {
                writeln!(f, "  [ skipped {:3} ]", skip)?;
            }
            skip = 0;
            writeln!(
                f,
                "  [ height: {:3}, len: {:5}, cap: {:5} ],",
                ix,
                q.len(),
                q.capacity()
            )?;
        }
        writeln!(f, "}}")?;
        Ok(())
    }
}

impl RecomputeHeap {
    pub fn new(max_height_allowed: usize) -> Self {
        let mut queues = Vec::with_capacity(max_height_allowed + 1);
        for _ in 0..max_height_allowed + 1 {
            queues.push(Default::default());
        }
        Self {
            queues: queues.into(),
            height_lower_bound: (max_height_allowed as i32 + 1).into(),
            length: 0.into(),
            swap: Default::default(),
        }
    }

    pub fn len(&self) -> usize {
        self.length.get()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn clear(&self) {
        for q in self.queues.borrow().iter() {
            q.borrow_mut().clear();
        }
        self.swap.borrow_mut().clear();
        self.length.set(0);
        self.height_lower_bound.set(i32::MAX);
    }

    pub fn link(&self, node: NodeRef) {
        // make these locals so the debugger can see them
        let _node_id = node.id().0;
        let node_height = node.height();
        assert!(node_height >= 0);
        assert!(node_height <= self.max_height_allowed());
        node.height_in_recompute_heap().set(node_height);
        let q = self.queue_for(node_height as usize);
        q.borrow_mut().push_back(node);
    }

    pub fn unlink(&self, node: &NodeRef) {
        let height_in_rch = node.height_in_recompute_heap().get();
        let queue = self.queue_for(height_in_rch as usize);
        // Unfortunately we must scan for the node
        // if this is slow, we should use a hash set or something instead with a fast "remove_any"
        // method
        let mut q = queue.borrow_mut();
        let Some(indexof) = q.iter().position(|x| crate::rc_thin_ptr_eq(x, node)) else {
            panic!("node was not in recompute heap: {node:?}");
        };
        // order within a particular queue does not matter at all.
        // they're all the same height so they cannot have any dependencies
        // so we can use swap_remove
        q.swap_remove_back(indexof);
    }

    pub fn insert(&self, node: NodeRef) {
        tracing::trace!("inserting into RCH @ h={} {:?}", node.height(), node);
        debug_assert!(
            !node.is_in_recompute_heap() && node.needs_to_be_computed(),
            "incorrect attempt to insert node in recompute heap"
        );
        debug_assert!(node.height() <= self.max_height_allowed());
        if node.height() < self.height_lower_bound.get() {
            self.height_lower_bound.set(node.height());
        }
        self.link(node);
        self.length.increment();
    }

    pub fn remove(&self, node: NodeRef) {
        let _node_id = node.id();
        debug_assert!(
            node.is_in_recompute_heap() && !node.needs_to_be_computed(),
            "incorrect attempt to remove node from recompute heap"
        );
        self.unlink(&node);
        node.height_in_recompute_heap().set(-1);
        self.length.decrement();
    }

    pub fn min_height(&self) -> i32 {
        self.raise_min_height();
        self.height_lower_bound.get()
    }

    fn raise_min_height(&self) {
        let queues = self.queues.borrow();
        let max = queues.len() as i32;
        if self.length.get() == 0 {
            self.height_lower_bound.set(max);
        } else {
            while queues
                .get(self.height_lower_bound.get() as usize)
                .map_or(false, |q| q.borrow().is_empty())
            {
                self.height_lower_bound.increment();
            }
        }
    }

    pub(crate) fn increase_height(&self, node: &NodeRef) {
        debug_assert!(node.height() > node.height_in_recompute_heap().get());
        debug_assert!(node.is_in_recompute_heap());
        debug_assert!(node.height() <= self.max_height_allowed());
        self.unlink(node);
        self.link(node.clone()); // sets height_in_recompute_heap <- height
    }

    fn queue_for(&self, height: usize) -> Ref<Queue> {
        Ref::map(self.queues.borrow(), |queue| &queue[height])
    }

    pub(crate) fn remove_min(&self) -> Option<NodeRef> {
        if self.is_empty() {
            return None;
        }
        let queues = self.queues.borrow();
        debug_assert!(self.height_lower_bound.get() >= 0);
        let len = queues.len();
        let mut queue;
        while {
            queue = queues.get(self.height_lower_bound.get() as usize)?;
            queue.borrow().is_empty()
        } {
            self.height_lower_bound.increment();
            debug_assert!(
                (self.height_lower_bound.get() as usize) < len,
                "RecomputeHeap::remove_min unexpectedly reached end of heap"
            );
        }
        let mut q = queue.borrow_mut();
        let node = q.pop_front()?;
        node.height_in_recompute_heap().set(-1);
        self.length.decrement();
        Some(node)
    }

    pub(crate) fn max_height_allowed(&self) -> i32 {
        self.queues.borrow().len() as i32 - 1
    }
    pub(crate) fn set_max_height_allowed(&self, new_max_height: usize) {
        let mut queues = self.queues.borrow_mut();
        #[cfg(debug_assertions)]
        {
            // this should be ensured by adjust-heights-heap's tracking of highest node seen.
            for i in new_max_height + 1..queues.len() {
                assert!(queues.get(i).is_none())
            }
        }
        queues.resize(new_max_height, Queue::default());
        self.height_lower_bound.set(std::cmp::min(
            self.height_lower_bound.get(),
            queues.len() as i32 + 1,
        ));
    }
}
