use super::NodeRef;
use std::collections::VecDeque;
use std::rc::Rc;

type Queue<'a> = VecDeque<NodeRef<'a>>;

#[derive(Debug)]
pub(crate) struct RecomputeHeap<'a> {
    queues: Vec<Queue<'a>>,
    height_lower_bound: i32,
    length: usize,
}

impl<'a> RecomputeHeap<'a> {
    pub fn new(max_height_allowed: usize) -> Self {
        let queues = vec![VecDeque::new(); max_height_allowed + 1];
        Self {
            queues,
            height_lower_bound: 0,
            length: 0,
        }
    }

    pub fn get_queue(&mut self, height: i32) -> Option<&mut Queue<'a>> {
        let h = if height < 0 {
            return None;
        } else {
            height as usize
        };
        Some(self.queue_for(h))
    }

    pub fn link(&mut self, node: NodeRef<'a>) {
        assert!(node.height() <= self.max_height_allowed());
        node.height_in_recompute_heap().set(node.height());
        let Some(q) = self.get_queue(node.height()) else { return };
        q.push_back(node);
    }

    pub fn unlink(&mut self, node: &NodeRef<'a>) {
        let Some(q) = self.get_queue(node.height_in_recompute_heap().get()) else { return };
        // Unfortunately we must scan for the node
        // if this is slow, we should use a hash set or something instead with a fast "remove_any"
        // method
        let Some(indexof) = q.iter().position(|x| Rc::ptr_eq(x, &node)) else { return };
        // order within a particular queue does not matter at all.
        // they're all the same height so they cannot have any dependencies
        // so we can use swap_remove
        q.swap_remove_back(indexof);
    }

    pub fn insert(&mut self, node: NodeRef<'a>) {
        debug_assert!(
            !node.is_in_recompute_heap() && node.needs_to_be_computed(),
            "incorrect attempt to insert node in recompute heap"
        );
        debug_assert!(node.height() <= self.max_height_allowed());
        if node.height() < self.height_lower_bound {
            self.height_lower_bound = node.height();
        }
        self.link(node);
        self.length += 1;
    }

    pub fn remove(&mut self, node: NodeRef<'a>) {
        debug_assert!(
            node.is_in_recompute_heap() && !node.needs_to_be_computed(),
            "incorrect attempt to remove node from recompute heap"
        );
        self.unlink(&node);
        node.height_in_recompute_heap().set(-1);
        self.length -= 1;
    }

    fn queue_for(&mut self, height: usize) -> &mut Queue<'a> {
        while height > self.queues.len() {
            self.queues.push(Queue::new());
        }
        self.queues
            .get_mut(height)
            .expect("we just created this queue!")
    }

    pub(crate) fn remove_min(&mut self) -> Option<NodeRef<'a>> {
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
    pub(crate) fn set_max_height_allowed(&mut self, new_max_height: usize) {
        #[cfg(debug_assertions)]
        {
            // this should be ensured by adjust-heights-heap's tracking of highest node seen.
            for i in new_max_height + 1..self.queues.len() {
                assert!(self.queues.get(i).is_none())
            }
        }
        self.queues.resize(new_max_height, VecDeque::new());
        self.height_lower_bound =
            std::cmp::min(self.height_lower_bound, self.queues.len() as i32 + 1);
    }
    pub(crate) fn increase_height(&mut self, node: &NodeRef<'a>) {
        debug_assert!(node.height() > node.height_in_recompute_heap().get());
        debug_assert!(node.is_in_recompute_heap());
        debug_assert!(node.height() <= self.max_height_allowed());
        self.unlink(node);
        self.link(node.clone()); // sets height_in_recompute_heap <- height
    }
}
