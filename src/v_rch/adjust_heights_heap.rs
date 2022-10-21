use crate::Invariant;

use super::recompute_heap::RecomputeHeap;
use super::NodeRef;
use std::collections::VecDeque;
use std::rc::{Rc, Weak};

type Queue<'a> = VecDeque<NodeRef<'a>>;

#[derive(Debug)]
pub(crate) struct AdjustHeightsHeap<'a> {
    length: usize,
    height_lower_bound: i32,
    max_height_seen: i32,
    queues: Vec<Queue<'a>>,
}

impl<'a> AdjustHeightsHeap<'a> {
    pub(crate) fn is_empty(&self) -> bool {
        self.length == 0
    }
    pub(crate) fn max_height_allowed(&self) -> i32 {
        self.queues.len() as i32 - 1
    }
    pub(crate) fn new(max_height_allowed: usize) -> Self {
        Self {
            length: 0,
            height_lower_bound: max_height_allowed as i32 + 1,
            max_height_seen: 0,
            queues: vec![Default::default(); max_height_allowed + 1],
        }
    }
    pub(crate) fn set_max_height_allowed(&mut self, new_mha: i32) {
        if new_mha < self.max_height_seen {
            panic!("cannot set max_height_allowed less than max height already seen");
        }
        debug_assert!(self.is_empty());
        debug_assert_eq!(calculate_len(&self.queues), 0);
        self.queues.resize(new_mha as usize, VecDeque::new());
    }
    pub(crate) fn add_unless_mem(&mut self, node: NodeRef<'a>) {
        if node.height_in_adjust_heights_heap().get() == -1 {
            let height = node.height();
            /* We process nodes in increasing order of pre-adjusted height, so it is a bug if we
            ever try to add a node that would violate that. */
            debug_assert!(height >= self.height_lower_bound);
            /* Whenever we set a node's height, we use [set_height], which enforces this. */
            debug_assert!(height <= self.max_height_allowed());
            node.height_in_adjust_heights_heap().set(height);
            self.length += 1;
            let q: &mut Queue<'a> = self.queues.get_mut(height as usize).unwrap();
            q.push_back(node);
        }
    }
    pub(crate) fn remove_min(&'_ mut self) -> Option<NodeRef<'a>> {
        if self.is_empty() {
            return None;
        }
        let mut height = self.height_lower_bound;
        let mut q: &mut Queue<'a>;
        while {
            q = self.queues.get_mut(height as usize)?;
            q.is_empty()
        } {
            height += 1;
        }
        self.height_lower_bound = height;
        let node = q.pop_front()?;
        node.height_in_adjust_heights_heap().set(-1);
        self.length -= 1;
        Some(node)
    }
    pub(crate) fn set_height(&mut self, node: &NodeRef<'a>, height: i32) {
        if height > self.max_height_seen {
            self.max_height_seen = height;
            if height > self.max_height_allowed() {
                panic!("node with too large height");
            }
        }
        let old_height = node.height();
        node.set_height(height);
        node.set_old_height(old_height);
    }
    pub(crate) fn ensure_height_requirement(
        &mut self,
        original_child: &NodeRef<'a>,
        original_parent: &NodeRef<'a>,
        child: &NodeRef<'a>,
        parent: &NodeRef<'a>,
    ) {
        debug_assert!(child.is_necessary());
        debug_assert!(parent.is_necessary());
        if Rc::ptr_eq(&parent, original_child) {
            panic!("adding edge made graph cyclic");
        }
        if child.height() >= parent.height() {
            self.add_unless_mem(parent.clone());
            /* We set [parent.height] after adding [parent] to the heap, so that [parent] goes
            in the heap with its pre-adjusted height. */
            self.set_height(parent, child.height() + 1);
        }
    }
    pub(crate) fn adjust_heights(
        &mut self,
        rch: &mut RecomputeHeap<'a>,
        original_child: NodeRef<'a>,
        original_parent: NodeRef<'a>,
    ) {
        println!(
            "adjust_heights from child(id={:?},h={:?}) to parent(id={:?},h={:?})",
            original_child.id(),
            original_child.height(),
            original_parent.id(),
            original_parent.height()
        );
        debug_assert!(self.is_empty());
        debug_assert!(original_child.height() >= original_parent.height());
        self.height_lower_bound = original_parent.height();
        self.ensure_height_requirement(
            &original_child,
            &original_parent,
            &original_child,
            &original_parent,
        );
        while let Some(child) = self.remove_min() {
            println!(
                "ahh popped(in_rch={:?}): {:?}",
                child.is_in_recompute_heap(),
                child
            );
            if child.is_in_recompute_heap() {
                rch.increase_height(&child, child.old_height());
            }
            let inner = child.inner();
            let ci = inner.borrow();
            if ci.num_parents() > 0 {
                for parent in ci.parents() {
                    let parent = parent.as_ref().and_then(Weak::upgrade).unwrap();
                    self.ensure_height_requirement(
                        &original_child,
                        &original_parent,
                        &child,
                        &parent,
                    );
                }
            }
            child.adjust_heights_bind_lhs_change(self, &original_child, &original_parent);
        }
        debug_assert!(self.is_empty());
        debug_assert!(original_child.height() < original_parent.height());
    }
}

fn calculate_len<'a>(queues: &[Queue<'a>]) -> usize {
    queues.iter().map(|q| q.len()).sum()
}

impl<'a> Invariant for AdjustHeightsHeap<'a> {
    fn invariant(&self) {
        assert_eq!(self.length, calculate_len(&self.queues));
        assert!(self.height_lower_bound >= 0);
        assert!(self.height_lower_bound as usize <= self.queues.len());
        for height in 0..self.height_lower_bound {
            let q = self.queues.get(height as usize).unwrap();
            assert!(q.is_empty())
        }
        assert!(self.max_height_seen >= 0);
        assert!(self.max_height_seen <= self.max_height_allowed());
        self.queues.invariant();
    }
}

impl<'a> Invariant for Vec<Queue<'a>> {
    fn invariant(&self) {
        let queues: &[Queue<'a>] = self.as_slice();
        for (height, q) in queues.iter().enumerate() {
            let q: &Queue<'a> = q;
            let height = height as i32;
            for node in q.iter() {
                assert!(node.height_in_adjust_heights_heap().get() == height);
            }
        }
    }
}
