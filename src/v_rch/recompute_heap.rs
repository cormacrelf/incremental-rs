use crate::v_rch::CellIncrement;

use super::NodeRef;
use std::cell::{Cell, Ref, RefCell, RefMut};
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
        writeln!(f, "  length: {},", self.length.get())?;
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

    pub fn clear(&self) {
        for q in self.queues.borrow().iter() {
            q.borrow_mut().clear();
        }
        self.swap.borrow_mut().clear();
        self.length.set(0);
        self.height_lower_bound.set(i32::MAX);
    }

    pub fn link(&self, node: NodeRef) {
        assert!(node.height() >= 0);
        assert!(node.height() <= self.max_height_allowed());
        node.height_in_recompute_heap().set(node.height());
        let q = self.queue_for(node.height() as usize);
        q.borrow_mut().push_back(node);
    }

    pub fn unlink(&self, node: &NodeRef) {
        let queue = self.queue_for(node.height_in_recompute_heap().get() as usize);
        // Unfortunately we must scan for the node
        // if this is slow, we should use a hash set or something instead with a fast "remove_any"
        // method
        let mut q = queue.borrow_mut();
        let Some(indexof) = q.iter().position(|x| crate::rc_thin_ptr_eq(x, node)) else { return };
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
        debug_assert!(
            node.is_in_recompute_heap() && !node.needs_to_be_computed(),
            "incorrect attempt to remove node from recompute heap"
        );
        self.unlink(&node);
        node.height_in_recompute_heap().set(-1);
        self.length.decrement();
    }

    pub fn min_height(&self) -> i32 {
        // using remove_min, this code would work.
        // but for us, we remove a whole layer at a time, and those nodes are gone.
        // so doing this will cause min_height to skip forward, and to
        // recompute_now some nodes that shouldn't be.
        //
        // if self.length.get() == 0 {
        //     self.height_lower_bound.set(self.queues.len() as i32);
        // } else {
        //     while {
        //         let q = self
        //             .queues[self.height_lower_bound.get() as usize].borrow();
        //         q.is_empty()
        //     } {
        //         self.height_lower_bound.set(self.height_lower_bound.get() + 1);
        //     }
        // }
        self.height_lower_bound.get()
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

    pub(crate) fn remove_min_layer(&self) -> Option<RefMut<VecDeque<NodeRef>>> {
        let queues = self.queues.borrow();
        if self.length.get() == 0 {
            return None;
        }
        debug_assert!(self.height_lower_bound.get() >= 0);
        let len = queues.len();
        let mut queue;
        while {
            queue = queues.get(self.height_lower_bound.get() as usize)?;
            queue.borrow().is_empty()
        } {
            self.height_lower_bound.increment();
            debug_assert!(self.height_lower_bound.get() as usize <= len);
        }
        let mut swap = self.swap.borrow_mut();
        swap.clear();
        let mut q = queue.borrow_mut();
        self.length.set(self.length.get() - q.len());
        // must use &mut *swap, because we don't want to swap the RefMuts,
        // we want to swap the actual VecDeques inside the RefMuts..
        std::mem::swap(&mut *swap, &mut *q);
        Some(swap)
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
