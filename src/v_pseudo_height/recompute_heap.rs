use super::node::ErasedNode;
use std::collections::VecDeque;
use std::rc::{Rc, Weak};

type StrongNode = Rc<dyn ErasedNode>;
type WeakNode = Weak<dyn ErasedNode>;
type Queue = VecDeque<Option<WeakNode>>;

#[derive(Debug)]
pub struct RecomputeHeap {
    queues: Vec<Queue>,
    height_lower_bound: i32,
    length: usize,
}

impl RecomputeHeap {
    pub fn new() -> Self {
        let queues = vec![VecDeque::new(); 100];
        Self {
            queues,
            height_lower_bound: 0,
            length: 0,
        }
    }

    pub fn insert(&mut self, weak_node: WeakNode) {
        println!("inserting node {:?}", weak_node);
        let Some(node) = weak_node.upgrade() else { return };
        let inner = node.inner();
        let mut i = inner.borrow_mut();
        let h = node.height();
        let Some(q) = self.get_queue(h) else { return };
        drop(q);
        node.height_in_recompute_heap().set(h);
        if h < self.height_lower_bound {
            self.height_lower_bound = node.height();
        }
        let Some(q) = self.get_queue(h) else { return };
        q.push_back(Some(weak_node));
        self.length += 1;
    }

    pub fn get_queue(&mut self, height: i32) -> Option<&mut Queue> {
        let h = if height < 0 {
            return None;
        } else {
            height as usize
        };
        Some(self.queue_for(h))
    }

    pub fn remove(&mut self, weak_node: WeakNode) {
        let Some(node) = weak_node.upgrade() else { return };
        if node.height_in_recompute_heap().get() < 0 {
            return;
        }
        let Some(q) = self.get_queue(node.height()) else { return };
        node.height_in_recompute_heap().set(-1);
        self.length -= 1;
    }

    fn queue_for(&mut self, height: usize) -> &mut Queue {
        while height > self.queues.len() {
            self.queues.push(Queue::new());
        }
        self.queues
            .get_mut(height)
            .expect("we just created this queue!")
    }

    pub(crate) fn remove_min(&mut self) -> Option<WeakNode> {
        if self.length == 0 {
            return None;
        }
        debug_assert!(self.height_lower_bound >= 0);
        let len = self.queues.len();
        let mut queue = self.queues.get_mut(self.height_lower_bound as usize)?;
        while queue.is_empty() {
            self.height_lower_bound += 1;
            queue = self.queues.get_mut(self.height_lower_bound as usize)?;
            debug_assert!(self.height_lower_bound as usize <= len);
        }
        queue.pop_front().flatten()
    }
}