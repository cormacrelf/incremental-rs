use super::node::ErasedNode;
use std::collections::VecDeque;
use std::rc::Rc;

type NodeRef = Rc<dyn ErasedNode>;
type Queue = VecDeque<NodeRef>;

#[derive(Debug)]
pub(crate) struct RecomputeHeap {
    queues: Vec<Queue>,
    height_lower_bound: i32,
    length: usize,
}

impl RecomputeHeap {
    pub fn new(max_height_allowed: usize) -> Self {
        let queues = vec![VecDeque::new(); max_height_allowed + 1];
        Self {
            queues,
            height_lower_bound: 0,
            length: 0,
        }
    }

    pub fn insert(&mut self, node: NodeRef) {
        let inner = node.inner();
        let h = node.height();
        let Some(q) = self.get_queue(h) else { return };
        drop(q);
        node.height_in_recompute_heap().set(h);
        if h < self.height_lower_bound {
            self.height_lower_bound = node.height();
        }
        self.link(node);
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

    pub fn link(&mut self, node: NodeRef) {
        let Some(q) = self.get_queue(node.height()) else { return };
        q.push_back(node);
    }
    pub fn unlink(&mut self, node: &NodeRef, at_height: i32) {
        let Some(q) = self.get_queue(at_height) else { return };
        // Unfortunately we must scan for the node
        // if this is slow, we should use a hash set or something instead with a fast "remove_any"
        // method
        let Some(indexof) = q.iter().position(|x| Rc::ptr_eq(x, &node)) else { return };
        // order within a particular queue does not matter at all.
        // they're all the same height so they cannot have any dependencies
        // so we can use swap_remove
        q.swap_remove_back(indexof);
    }

    pub fn remove(&mut self, node: NodeRef) {
        if node.height_in_recompute_heap().get() < 0 {
            return;
        }
        self.unlink(&node, node.height());
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

    pub(crate) fn remove_min(&mut self) -> Option<NodeRef> {
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
        let removed = queue.pop_front()?;
        removed.height_in_recompute_heap().set(-1);
        Some(removed)
    }
    pub(crate) fn max_height_allowed(&self) -> i32 {
        self.queues.len() as i32 - 1
    }
    pub(crate) fn increase_height(&mut self, node: &NodeRef, old_height: i32) {
        debug_assert!(node.old_height() >= 0);
        debug_assert!(node.height() > node.old_height());
        debug_assert!(node.height() > node.height_in_recompute_heap().get());
        debug_assert!(node.height() <= self.max_height_allowed());
        debug_assert!(node.is_in_recompute_heap());
        self.unlink(node, old_height);
        self.link(node.clone());
        node.set_old_height(-1);
    }
}
