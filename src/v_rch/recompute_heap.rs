use crate::v_rch::CellIncrement;

use super::NodeRef;
use std::cell::{Cell, RefCell, RefMut};
use std::collections::VecDeque;

type Queue = RefCell<VecDeque<NodeRef>>;

#[derive(Debug)]
pub(crate) struct RecomputeHeap {
    queues: Vec<Queue>,
    height_lower_bound: Cell<i32>,
    length: Cell<usize>,
    swap: Queue,
}

impl RecomputeHeap {
    pub fn new(max_height_allowed: usize) -> Self {
        let mut queues = Vec::with_capacity(max_height_allowed + 1);
        for _ in 0..max_height_allowed + 1 {
            queues.push(Default::default());
        }
        Self {
            queues,
            height_lower_bound: (max_height_allowed as i32 + 1).into(),
            length: 0.into(),
            swap: Default::default(),
        }
    }

    pub fn clear(&self) {
        for q in self.queues.iter() {
            q.borrow_mut().clear();
        }
        self.swap.borrow_mut().clear();
        self.length.set(0);
        self.height_lower_bound.set(i32::MAX);
    }

    pub fn get_queue(&self, height: i32) -> Option<&Queue> {
        let h = if height < 0 {
            return None;
        } else {
            height as usize
        };
        Some(self.queue_for(h))
    }

    pub fn link(&self, node: NodeRef) {
        assert!(node.height() <= self.max_height_allowed());
        node.height_in_recompute_heap().set(node.height());
        let Some(q) = self.get_queue(node.height()) else { return };
        q.borrow_mut().push_back(node);
    }

    pub fn unlink(&self, node: &NodeRef) {
        let Some(queue) = self.get_queue(node.height_in_recompute_heap().get()) else { return };
        // Unfortunately we must scan for the node
        // if this is slow, we should use a hash set or something instead with a fast "remove_any"
        // method
        let mut q = queue.borrow_mut();
        let Some(indexof) = q.iter().position(|x| crate::rc_fat_ptr_eq(x, node)) else { return };
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

    fn queue_for(&self, height: usize) -> &Queue {
        self.queues
            .get(height)
            .expect("we just created this queue!")
    }

    pub(crate) fn remove_min_layer(&self) -> Option<RefMut<VecDeque<NodeRef>>> {
        if self.length.get() == 0 {
            return None;
        }
        debug_assert!(self.height_lower_bound.get() >= 0);
        let len = self.queues.len();
        let mut queue;
        while {
            queue = self.queues.get(self.height_lower_bound.get() as usize)?;
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
        self.queues.len() as i32 - 1
    }
    pub(crate) fn set_max_height_allowed(&mut self, new_max_height: usize) {
        #[cfg(debug_assertions)]
        {
            // this should be ensured by adjust-heights-heap's tracking of highest node seen.
            for i in new_max_height + 1..self.queues.len() {
                assert!(self.queues.get(i).is_none())
            }
        }
        self.queues.resize(new_max_height, Queue::default());
        self.height_lower_bound.set(std::cmp::min(
            self.height_lower_bound.get(),
            self.queues.len() as i32 + 1,
        ));
    }
}
