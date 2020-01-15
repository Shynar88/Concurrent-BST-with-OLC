//! Concurrent binary search tree protected with optimistic lock coupling.
//!
//! - From Bronson, Casper, Chafi, Olukotun. A Practical Concurrent Binary Search Tree. PPoPP 2010
//! (https://stanford-ppl.github.io/website/papers/ppopp207-bronson.pdf)
//!
//! - We implement partially external relaxed tree (section 3) with a few simplifications.

use core::cmp;
use core::mem::{self, ManuallyDrop};
use core::sync::atomic::Ordering;
use crossbeam_epoch::{unprotected, Atomic, Guard, Owned, Shared};
use lock::seqlock::{ReadGuard, SeqLock};

mod base;

use crate::map::ConcurrentMap;
pub use base::Bst;
use base::{AtomicRW, Cursor, Dir, Node, NodeInner};

impl<'g, K: Ord, V> Cursor<'g, K, V> {
    /// Discards the current node.
    fn pop(&mut self) -> Result<(), ()> {
        if self.is_root() {
            return Err(());
        } else {
            let (cur, d) = self.ancestors.pop().unwrap();
            self.current = cur;
            self.dir = d;
            let seq_lock = unsafe {&self.current.deref().inner}; 
            unsafe { self.guard.atomic_write(ManuallyDrop::new(seq_lock.read_lock()));}
            return Ok(());
        }
    }

    /// Pushs a new node as the current one.
    ///
    /// Returns `Err(())` if the existing current node's guard is invalidated.
    fn push(
        &mut self,
        current: Shared<'g, Node<K, V>>,
        guard: ReadGuard<'g, NodeInner<K, V>>,
        dir: Dir,
    ) -> Result<(), ()> {
        if self.guard.validate() {
            self.ancestors.push((self.current, self.dir));
            let new_guard = ManuallyDrop::new(guard);
            unsafe{ self.guard.atomic_swap(new_guard)};
            self.dir = dir;
            self.current = current.with_tag(0); 
            return Ok(());
        } else {
            self.guard.restart();
            mem::forget(guard);
            return Err(());
        }
    }

    /// Finds the given `key` from the current cursor (`self`).
    ///
    /// - Returns `Ordering::Less` or `Ordering::Greater` if the key should be inserted from the
    ///   left or right (resp.)  child of the resulting cursor.
    /// - Returns `Ordering::Equal` if the key was found.
    fn find(&mut self, key: &K, guard: &'g Guard) -> cmp::Ordering {
        loop {
            let child_ptr = self.guard.child(self.dir);
            let child = child_ptr.load(Ordering::Relaxed, guard);
            if child == Shared::null() {
                match self.dir {
                    Dir::L => {
                        return cmp::Ordering::Less;
                    },
                    Dir::R => {
                        return cmp::Ordering::Greater;
                    },
                }
            } else {
                let child_key = unsafe {&child.deref().key}; 
                let read_guard = unsafe{ child.deref().inner.read_lock() };
                if child_key == key {
                    match self.push(child, read_guard, Dir::R) {  
                        Ok(()) => {
                        },
                        Err(()) => {
                            continue;
                        },
                    }
                    return cmp::Ordering::Equal;
                } else if key > child_key {
                    match self.push(child, read_guard, Dir::R) {
                        Ok(()) => {
                        },
                        Err(()) => {
                            continue;
                        },
                    }
                } else {
                    match self.push(child, read_guard, Dir::L) {
                        Ok(()) => {

                        },
                        Err(()) => {
                            continue;
                        },
                    }
                }
            }
        }
    }

    // Recursively tries to unlink `self.current` if it's vacant and at least one of children is
    // null.
    //
    // You should repeat cleanup until the current `self.current` is no longer cleanup-able.
    fn cleanup(&mut self, guard: &Guard) { 
        match &self.guard.value{ 
            None => {
                if self.is_root() {
                    //do nothing, node is root
                    return;
                }
                //node is vacant
                let right_child = self.guard.right.load(Ordering::Relaxed, guard);
                let left_child = self.guard.left.load(Ordering::Relaxed, guard);
                if (right_child != Shared::null()) && (left_child != Shared::null()) {
                    //do nothing, it has 2 children 
                    return;
                }
                //call pop and go into function again; do cleanup
                match ManuallyDrop::into_inner(self.guard.clone()).upgrade() {
                    Ok(write_guard) => {
                        let A_node = self.current; 
                        let A_guard = write_guard;
                        match self.pop() {
                            Ok(()) => {

                            },
                            Err(()) => {
                                return;
                            }
                        }
                        if self.current.tag() != 0 {
                            return;
                        } 
                        let write_guard = unsafe{ self.current.deref().inner.write_lock() }; 
                        if (right_child == Shared::null()) &&  (left_child == Shared::null()){
                            unsafe { (*write_guard).child(self.dir).atomic_write(Atomic::null());}
                        } else if left_child == Shared::null() {
                            A_guard.right.store(right_child.with_tag(1), Ordering::Relaxed);
                            (*write_guard).child(self.dir).store(right_child, Ordering::Relaxed);  
                        } else { 
                            // if right_child is Shared::null()
                            A_guard.left.store(left_child.with_tag(1), Ordering::Relaxed);
                            (*write_guard).child(self.dir).store(left_child, Ordering::Relaxed);
                        }
                        unsafe { guard.defer_destroy(A_node)}; 
                        self.cleanup(guard);
                    },
                    Err(()) => {
                    },
                }
            }
            Some(_v) => {
                //do nothing, node is not vacant
                return;
            }
        }
    }
}

impl<K: Ord, V> ConcurrentMap<K, V> for Bst<K, V>
where
    K: Clone,
    Option<V>: AtomicRW,
{
    /// Inserts the given `value` at the given `key`.
    ///
    /// - Returns `Ok(())` if `value` is inserted.
    /// - Returns `Err(value)` for the given `value` if `key` is already occupied.
    fn insert<'a>(&'a self, key: &'a K, value: V, guard: &'a Guard) -> Result<(), V> {
        loop {
            let mut cursor = self.cursor(guard);
            let ordering = cursor.find(key, guard);
            match ManuallyDrop::into_inner(cursor.guard.clone()).upgrade() {
                Ok(write_guard) => {
                    if ordering == cmp::Ordering::Equal {
                        if write_guard.value.is_none() {
                            unsafe { write_guard.value.atomic_write(Some(value));}
                            return Ok(());
                        }
                        return Err(value);
                    } else if ordering == cmp::Ordering::Less {
                        let new_node = Atomic::new(Node {
                            key: key.clone(),
                            inner: SeqLock::new(NodeInner {
                                value: Some(value),
                                left: Atomic::null(),
                                right: Atomic::null(),
                            }),
                        });
                        unsafe { write_guard.left.atomic_write(new_node);}
                        return Ok(());
                    } else {
                        let new_node = Atomic::new(Node {
                            key: key.clone(),
                            inner: SeqLock::new(NodeInner {
                                value: Some(value),
                                left: Atomic::null(),
                                right: Atomic::null(),
                            }),
                        });
                        unsafe { write_guard.right.atomic_write(new_node);}
                        return Ok(());
                    }
                },
                Err(()) => {
                    // retry from beginning
                },
            }
        }
    }

    /// Deletes the given `key`.
    ///
    /// - Returns `Ok(value)` if `value` was deleted from `key`.
    /// - Returns `Err(())` if `key` was vacant.
    fn delete(&self, key: &K, guard: &Guard) -> Result<V, ()> {
        loop {
            let mut cursor = self.cursor(guard);
            let ordering = cursor.find(key, guard);
            match ManuallyDrop::into_inner(cursor.guard.clone()).upgrade() {
                Ok(write_guard) => {
                    if ordering == cmp::Ordering::Equal {
                        if write_guard.value.is_none() {
                            return Err(());
                        }
                        let prev_value = unsafe{(write_guard.value.atomic_swap(None)).unwrap()};
                        cursor.cleanup(guard);
                        return Ok(prev_value);
                    } else if ordering == cmp::Ordering::Less {
                        return Err(()); 
                    } else {
                        return Err(()); 
                    }
                },
                Err(()) => {
                    // retry from beginning
                },
            }
        }
    }

    /// Looks up the given `key` and calls `f` for the found `value`.
    fn lookup<'a, F, R>(&'a self, key: &'a K, guard: &'a Guard, f: F) -> R
    where
        F: FnOnce(Option<&V>) -> R,
    {
        loop {
            let mut cursor = self.cursor(guard);
            if cursor.find(key, guard) == cmp::Ordering::Equal {
                match ManuallyDrop::into_inner(cursor.guard.clone()).upgrade() {
                    Ok(write_guard) => {
                        return f(write_guard.value.as_ref());
                    },
                    Err(()) => {
                        // retry from beginning
                    },
                }
            } else {
                return f(None);
            }
        }
    }
}

impl<K: Ord, V> Drop for Bst<K, V> {
    fn drop(&mut self) {
        // iterative in order tree traversal 
        let guard = crossbeam_epoch::pin();
        let mut stack = Vec::new();
        let mut current = self.root.load(Ordering::Relaxed, &guard);  
        loop {
            if current != Shared::null() {
                stack.push(current);
                let write_guard = unsafe{ current.deref().inner.write_lock() };  
                current = write_guard.left.load(Ordering::Relaxed, &guard);
            } else if stack.len() != 0 {
                current = stack.pop().unwrap(); 
                let write_guard = unsafe{ current.deref().inner.write_lock() };
                unsafe { guard.defer_destroy(current) };  
                current = write_guard.right.load(Ordering::Relaxed, &guard);
            } else {
                break;
            } 
        }
    }
}
