use std::{io::Empty, ops::{Range, RangeBounds}};


pub(super) trait IntervalTreeEntry: std::fmt::Debug + std::cmp::Eq {
    fn interval(&self) -> Range<u64>;
}

/// An interval tree.
///
/// Entries are values that have an associated interval.  Multiple
/// entries can have the same exact interval, but must be unique
/// as a whole.
pub(super) struct ItervalTree<T: IntervalTreeEntry> {
    span: Range<u64>,
    root_node: NodeType<T>
}

impl <T: IntervalTreeEntry> ItervalTree<T> {
    pub fn new(span: Range<u64>) -> Self {
        assert!(span.start >= 0);
        assert!(span.start < span.end);
        Self {
            span,
            root_node: NodeType::Empty
        }
    }

    /// Inserts entry, returns true if entry was added (did not already exist in the tree).
    pub fn insert(&mut self, entry: T) -> bool {
        assert!(!entry.interval().is_empty());
        self.root_node.insert(&self.span, entry)
    }

    pub fn find<'a>(&'a self, query_span: &Range<u64>) -> Vec<&'a T> {
        assert!(!query_span.is_empty());
        let mut result: Vec<&T> = Vec::new();
        self.root_node.find(&self.span, query_span, &mut result);
        result
    }

    pub fn is_empty(&self) -> bool {
        self.root_node.is_empty()
    }
}

#[derive(Debug)]
enum NodeType<T: IntervalTreeEntry> {
    Empty,
    Populated(Box<BinaryNode<T>>),
}

impl <T: IntervalTreeEntry> NodeType<T> {


    fn insert(&mut self, self_span: &Range<u64>, entry: T) -> bool {
        assert!(self_span.contains_range(&entry.interval()));

        match self {
            NodeType::Empty => {
                *self = NodeType::Populated(Box::new(BinaryNode { 
                    here: vec![entry], 
                    left_node: NodeType::Empty, 
                    right_node: NodeType::Empty 
                }));
                return true;
            },
            NodeType::Populated(p) => {
                let was_inline_single = p.is_inline_singleton();

                let mid = (self_span.start + self_span.end) / 2;
                let left_span = self_span.start..mid;
                let right_span = mid..self_span.end;

                let can_have_children = (self_span.end - self_span.start) >= 2;

                let result = match can_have_children {
                    true if left_span.contains_range(&entry.interval()) => {
                        p.left_node.insert(&left_span, entry)
                    },
                    true if right_span.contains_range(&entry.interval()) => {
                        p.right_node.insert(&right_span, entry)
                    },
                    _ => {
                        if p.here.contains(&entry) {
                            false
                        } else {
                            p.here.push(entry);
                            true
                        }
                    }
                };

                if was_inline_single && result {
                    // Node was a special inlined singleton, check if inlined entry needs re-located.
                    let singleton_entry = p.here.swap_remove(0);
                    assert!(self.insert(self_span, singleton_entry));
                }

                result
            },
        }

    }
    
    fn find<'a>(&'a self, self_span: &Range<u64>, query_span: &Range<u64>, result: &mut Vec<&'a T>) {
        assert!(self_span.overlaps_range(query_span));

        match self {
            NodeType::Empty => {
                return;
            },
            NodeType::Populated(p) => {
                for h in &p.here {
                    if query_span.overlaps_range(&h.interval()) {
                        result.push(h);
                    }
                }

                if (self_span.end - self_span.start) >= 2 {
                    let mid = (self_span.start + self_span.end) / 2;
                    let left_span = self_span.start..mid;
                    let right_span = mid..self_span.end;
    
                    if left_span.overlaps_range(query_span) {
                        p.left_node.find(&left_span, query_span, result);
                    }
                    if right_span.overlaps_range(query_span) {
                        p.right_node.find(&right_span, query_span, result);
                    }
                }
            },
        }

    }

    fn remove(&mut self, self_span: &Range<u64>, entry: &T) -> bool {
        assert!(self_span.contains_range(&entry.interval()));

        match self {
            NodeType::Empty => {
                return false;
            },
            NodeType::Populated(p) => {
                if p.is_inline_singleton() {
                    // Special case for inline singletons
                    if p.here.first().unwrap() == entry {
                        *self = NodeType::Empty;
                        return true;
                    } else {
                        return false;
                    }
                }

                let mid = (self_span.start + self_span.end) / 2;
                let left_span = self_span.start..mid;
                let right_span = mid..self_span.end;


                let can_have_children = (self_span.end - self_span.start) >= 2;

                let result: bool = match can_have_children {
                    true if left_span.contains_range(&entry.interval()) => {
                        p.left_node.remove(&left_span, entry)
                    },
                    true if right_span.contains_range(&entry.interval()) => {
                        p.right_node.remove(&right_span, entry)
                    },
                    _ => {
                        if let Some(idx) = p.here.iter().position(|x| x == entry) {
                            p.here.swap_remove(idx);
                            debug_assert!(!p.here.contains(entry));
                            true
                        } else {
                            false
                        }
                    }
                };

                if result {
                    // something was removed so, consider demotion
                    match (p.here.is_empty(), &mut p.left_node, &mut p.right_node) {
                        (false, _, _) => {
                            // nothing to demote
                        }
                        (true, NodeType::Empty, NodeType::Empty) => {
                            panic!("Node is entirely empty, should be unreachable")
                        },
                        (true, NodeType::Populated(lp), NodeType::Empty) => {
                            if lp.is_inline_singleton() {
                                p.here.push(lp.here.swap_remove(0));
                                p.left_node = NodeType::Empty;
                            }
                        },
                        (true, NodeType::Empty, NodeType::Populated(rp)) => {
                            if rp.is_inline_singleton() {
                                p.here.push(rp.here.swap_remove(0));
                                p.right_node = NodeType::Empty;
                            }
                        },
                        _ => {}
                    }
                }

                result
            },
        }
    }




    
    fn is_empty(&self) -> bool {
        match self {
            NodeType::Empty => true,
            NodeType::Populated(p) => {
                debug_assert!(!(p.here.is_empty() && p.left_node.is_empty() && p.right_node.is_empty()));
                false
            },
        }
    }
}

#[derive(Debug)]
struct BinaryNode<T: IntervalTreeEntry> {
    here: Vec<T>,
    left_node: NodeType<T>,
    right_node: NodeType<T>
}


impl <T: IntervalTreeEntry> BinaryNode<T> {
    fn is_inline_singleton(&self) -> bool {
        self.left_node.is_empty() && 
        self.right_node.is_empty() &&
        {
            assert!(!self.here.is_empty());
            self.here.len() == 1
        }
    }
}

trait RangeOps<T> {
    fn contains_range(&self, other: &Range<T>) -> bool;
    fn overlaps_range(&self, other: &Range<T>) -> bool;
}

impl <T: PartialOrd> RangeOps<T> for Range<T> {
    fn contains_range(&self, other: &Range<T>) -> bool {
        (self.start <= other.start) && (other.end <= self.end)
    }
    fn overlaps_range(&self, other: &Range<T>) -> bool {
        (self.end <= other.start) == (other.end <= self.start)
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::btree_set::Intersection, ops::Range};

    use crate::tests::init_logger;

    use super::{IntervalTreeEntry, ItervalTree};

    #[derive(Debug, Eq, PartialEq, Clone)]
    struct Entry {
        span: Range<u64>,
        value: String
    }

    impl Entry {
        fn new(span: Range<u64>, value: &str) -> Self {
            Self {
                span: span,
                value: value.to_string(),
            }
        }
    }

    impl IntervalTreeEntry for Entry {
        fn interval(&self) -> Range<u64> {
            self.span.clone()
        }
    }

    #[test]
    fn simple() {
        init_logger();

        let mut tree: ItervalTree<Entry> = ItervalTree::new(0..100);

        let hello = Entry::new(40..50, "Hello");
        let world = Entry::new(45..60, "World");
        tree.insert(hello.clone());
        tree.insert(world.clone());

        assert!(tree.find(&(0..5)).is_empty());
        assert_eq!(tree.find(&(40..41)), vec![&hello]);

    }
}


