// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Leonardo Lopes Pereira <leonardolopespereira@outlook.com>

//! An intrusive tail queue: a two-word head and two-word links, walked
//! both ways, whose links live inside caller-owned nodes, so the queue
//! never allocates or frees.  The crate is `no_std` and needs no `alloc`.
//!
//! ```
//! use tailq::{Link, TailQueue};
//!
//! struct Thread {
//!     id: u32,
//!     link: Link,
//! }
//!
//! tailq::adapter!(RunLink = Thread { link });
//! type RunQueue<'nodes> = TailQueue<'nodes, RunLink>;
//!
//! let mut first = Thread { id: 1, link: Link::new() };
//! let mut second = Thread { id: 2, link: Link::new() };
//! let mut run_queue = RunQueue::new();
//! run_queue.push_back(&mut first);
//! run_queue.push_front(&mut second);
//!
//! let ids: Vec<u32> = run_queue.iter().map(|thread| thread.id).collect();
//! assert_eq!(ids, [2, 1]);
//!
//! let next = run_queue.pop_front();
//! assert!(next.is_some_and(|thread| thread.id == 2 && !thread.link.is_linked()));
//! ```
//!
//! [`adapter!`] declares the [`Adapter`] that links a node type through
//! one of its fields, and a queue type names it; one node joins several
//! queues at once through several fields.
//!
//! The head holds the first node's link and the last node's.  Each node's
//! link holds the next node's link and the previous node's, or, for the
//! first node, a marker that stands for the head and is never
//! dereferenced.  So a backward step is one load and one compare, any
//! node leaves in O(1) given the head, and nothing points into the head:
//! it moves freely, full or empty, like any other value.  Pushing at
//! either end, linking before or after a cursor, and appending another
//! queue are O(1) too.  A head of all zero bytes is an empty queue, so
//! heads inside zeroed memory need no initialisation.
//!
//! A queue keeps no length unless its third parameter is [`Counted`]:
//! `TailQueue<'nodes, A, Counted>` adds a word to the head and answers
//! [`len`](TailQueue::len) in O(1); the default, [`Uncounted`], keeps the
//! head at two words.
//!
//! Nodes are caller-owned and embed a [`Link`].  A queue
//! `TailQueue<'nodes, A>`, with `A` the [`Adapter`] that finds the link,
//! takes each node as `&'nodes mut A::Node` and
//! hands it back the same way when it leaves, so the borrow checker keeps
//! a linked node alive, in place, reached only through the queue, and on
//! one queue at a time.  The `_ptr` operations take and return raw
//! pointers instead, for nodes whose lifetime no `'nodes` describes; the
//! pointers they return are the ones the nodes were pushed with, so they
//! may write the nodes wherever those pushes allowed it.  The ones that
//! take a pointer are `unsafe` and their callers keep the queue's promises
//! by hand; naming a node by its address is `unsafe` too, since nothing in
//! a link says which queue holds it.
//!
//! A node is on a queue from the push that links it until it leaves: by
//! removal, by replacement, by [`clear`](TailQueue::clear), or by the
//! queue being dropped; [`append`](TailQueue::append) moves it to another
//! queue without leaving.  Leaving marks the link as on no queue, which
//! [`Link::is_linked`] reads; a queue that is forgotten, or overwritten
//! without being dropped, leaves its nodes marked as linked.  Debug builds
//! check, before an operation writes anything, that a link's neighbours
//! point back at it and that a link at either end is this queue's, and
//! that a pushed link is on no queue; they also fill a leaving link's
//! forward word with an address no link can have.

#![no_std]

mod adapter;
#[cfg(doctest)]
mod compile_fail;
mod count;
mod cursor;

pub use adapter::{Adapter, Payload};
pub use count::{Count, Counted, Uncounted};
pub use cursor::{Cursor, CursorMut, Iter, IterMut, IterPtr};

use core::cell::Cell;
use core::fmt;
use core::marker::PhantomData;
use core::mem::size_of;
use core::ptr::{self, NonNull};

/// An address no link can have: a link there would run past the end of
/// the address space.  The first node's `prev` holds it in place of the
/// head, and debug builds fill a leaving link's `next` with it, so that a
/// stale walk faults on a recognisable address.  It is never
/// dereferenced.
// SAFETY: `usize::MAX` is not zero.
const NOWHERE: NonNull<Link> =
    unsafe { NonNull::new_unchecked(ptr::without_provenance_mut(usize::MAX)) };

/// The field a node embeds to join one [`TailQueue`].
///
/// Two words: the next node's link, and the previous node's, `None`
/// while the link is on no queue.  A link of all zero bytes is the one
/// [`new`](Self::new) returns.
#[repr(C)]
#[derive(Debug)]
pub struct Link {
    next: Cell<Option<NonNull<Self>>>,
    /// The previous node's link, or [`NOWHERE`] for the first node; `None`
    /// while on no queue.
    prev: Cell<Option<NonNull<Self>>>,
}

impl Link {
    /// Returns a link for a node that is on no queue.
    #[must_use]
    #[inline]
    pub const fn new() -> Self {
        Self {
            next: Cell::new(None),
            prev: Cell::new(None),
        }
    }

    /// Returns whether the link is on a queue.
    ///
    /// A link left on a queue that was forgotten, or overwritten without
    /// being dropped, still reads as linked.
    #[must_use]
    #[inline]
    pub const fn is_linked(&self) -> bool {
        self.prev.get().is_some()
    }

    /// Returns the previous node's link, or `None` for the first node.
    ///
    /// # Safety
    ///
    /// The link must be on a queue.
    #[inline]
    unsafe fn prev_link(&self) -> Option<NonNull<Self>> {
        // SAFETY: a link on a queue has a `prev`.
        let prev = unsafe { self.prev.get().unwrap_unchecked() };
        (prev != NOWHERE).then_some(prev)
    }
}

impl Default for Link {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

// SAFETY: a link's words are private to this crate.  The queue holding
// its node writes them only under `&mut TailQueue`, which excludes every
// reference the queue handed out; reads through other references, as
// `is_linked` and `Debug` make, are ordered with those writes by that
// borrow, or, for a node pushed by pointer, by the push contract.
unsafe impl Send for Link {}
// SAFETY: as for `Send`: shared access only reads the words, and every
// write is ordered with it.
unsafe impl Sync for Link {}

const _: () = assert!(
    size_of::<Link>() == 2 * size_of::<usize>(),
    "a link is two words",
);

/// The head of a tail queue of `A::Node`s borrowed for `'nodes`, which
/// keeps its length when `C` is [`Counted`].
///
/// Nothing points into a head, so it moves freely, full or empty.  A
/// head of all zero bytes is an empty queue.  Dropping a head marks the
/// links of the nodes still on it as on no queue, so those nodes must be
/// live then.
pub struct TailQueue<'nodes, A: Adapter, C: Count = Uncounted> {
    /// The first node's link, `None` when empty.
    first: Option<NonNull<Link>>,
    /// The last node's link, `None` when empty.
    last: Option<NonNull<Link>>,
    count: C,
    nodes: PhantomData<(A, &'nodes mut A::Node)>,
}

impl<'nodes, A: Adapter, C: Count> TailQueue<'nodes, A, C> {
    /// Returns an empty queue.
    #[must_use]
    #[inline]
    pub const fn new() -> Self {
        Self {
            first: None,
            last: None,
            count: C::ZERO,
            nodes: PhantomData,
        }
    }

    /// Returns whether the queue holds no nodes.
    #[must_use]
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.first.is_none()
    }

    /// Empties the queue in O(n), marking every node's link as on no
    /// queue.  The nodes pushed as `&'nodes mut` stay borrowed until
    /// `'nodes` ends.
    #[inline]
    pub fn clear(&mut self) {
        self.release_all();
    }

    /// Returns the first node, or `None` when empty.
    #[must_use]
    #[inline]
    pub fn front(&self) -> Option<&A::Node> {
        // SAFETY: a node stays live while it is on the queue.
        self.front_ptr().map(|node| unsafe { node.as_ref() })
    }

    /// Returns the last node, or `None` when empty.
    #[must_use]
    #[inline]
    pub fn back(&self) -> Option<&A::Node> {
        // SAFETY: a node stays live while it is on the queue.
        self.back_ptr().map(|node| unsafe { node.as_ref() })
    }

    /// Returns the pointer the first node was pushed with, or `None` when
    /// empty.
    ///
    /// It names the node only while the node is on the queue.  It may
    /// write the node wherever its push allowed it, except the link, and
    /// only while no reference the queue handed out to the node is alive.
    #[must_use]
    #[inline]
    pub fn front_ptr(&self) -> Option<NonNull<A::Node>> {
        // SAFETY: a node stays live while it is on the queue.
        self.first.map(|link| unsafe { A::node(link) })
    }

    /// Returns the pointer the last node was pushed with, or `None` when
    /// empty, as [`front_ptr`](Self::front_ptr) does.
    #[must_use]
    #[inline]
    pub fn back_ptr(&self) -> Option<NonNull<A::Node>> {
        // SAFETY: a node stays live while it is on the queue.
        self.last.map(|link| unsafe { A::node(link) })
    }

    /// Returns an iterator over the nodes, front to back; reversed, back
    /// to front.
    #[must_use]
    #[inline]
    pub const fn iter(&self) -> Iter<'_, A> {
        Iter::new(self.iter_ptr())
    }

    /// Returns an iterator over the pointers the nodes were pushed with,
    /// front to back; reversed, back to front, each as
    /// [`front_ptr`](Self::front_ptr) returns it.
    #[must_use]
    #[inline]
    pub const fn iter_ptr(&self) -> IterPtr<'_, A> {
        IterPtr::new(self.first, self.last)
    }

    /// Returns a cursor at the first node, or at the ghost when empty.
    #[must_use]
    #[inline]
    pub const fn cursor_front(&self) -> Cursor<'_, 'nodes, A, C> {
        Cursor::new(self, self.first)
    }

    /// Returns a cursor at the last node, or at the ghost when empty.
    #[must_use]
    #[inline]
    pub const fn cursor_back(&self) -> Cursor<'_, 'nodes, A, C> {
        Cursor::new(self, self.last)
    }

    /// Returns a cursor at `node`.
    ///
    /// # Panics
    ///
    /// In debug builds, when the node's link is on no queue, or when it is
    /// the first link of a queue other than this one.
    ///
    /// # Safety
    ///
    /// `node` must be on this queue.
    #[must_use]
    #[inline]
    pub unsafe fn cursor_from_ptr(
        &self,
        node: NonNull<A::Node>,
    ) -> Cursor<'_, 'nodes, A, C> {
        // SAFETY: the caller puts the node on this queue.
        let current = unsafe { self.linked(A::link(node)) };
        Cursor::new(self, Some(current))
    }

    /// Returns a mutable cursor at the first node, or at the ghost when
    /// empty.
    #[must_use]
    #[inline]
    pub const fn cursor_front_mut(&mut self) -> CursorMut<'_, 'nodes, A, C> {
        let first = self.first;
        CursorMut::new(self, first)
    }

    /// Returns a mutable cursor at the last node, or at the ghost when
    /// empty.
    #[must_use]
    #[inline]
    pub const fn cursor_back_mut(&mut self) -> CursorMut<'_, 'nodes, A, C> {
        let last = self.last;
        CursorMut::new(self, last)
    }

    /// Returns a mutable cursor at `node`.
    ///
    /// # Panics
    ///
    /// In debug builds, when the node's link is on no queue, or when it is
    /// the first link of a queue other than this one.
    ///
    /// # Safety
    ///
    /// `node` must be on this queue.
    #[must_use]
    #[inline]
    pub unsafe fn cursor_mut_from_ptr(
        &mut self,
        node: NonNull<A::Node>,
    ) -> CursorMut<'_, 'nodes, A, C> {
        // SAFETY: the caller puts the node on this queue.
        let current = unsafe { self.linked(A::link(node)) };
        CursorMut::new(self, Some(current))
    }

    /// Pushes `node` at the front.
    ///
    /// # Panics
    ///
    /// In debug builds, when the node's link is marked as on a queue.
    #[inline]
    pub fn push_front(&mut self, node: &'nodes mut A::Node) {
        // SAFETY: the `'nodes` borrow keeps the node live, unmoved and
        // reached only through the queue, a `&mut` allows writes to all
        // of it, and it is on no other queue.
        unsafe { self.push_front_ptr(NonNull::from(node)) };
    }

    /// Pushes `node` at the back.
    ///
    /// # Panics
    ///
    /// In debug builds, when the node's link is marked as on a queue.
    #[inline]
    pub fn push_back(&mut self, node: &'nodes mut A::Node) {
        // SAFETY: the `'nodes` borrow keeps the node live, unmoved and
        // reached only through the queue, a `&mut` allows writes to all
        // of it, and it is on no other queue.
        unsafe { self.push_back_ptr(NonNull::from(node)) };
    }

    /// Pushes the node at `node` at the front.
    ///
    /// # Panics
    ///
    /// In debug builds, when the node's link is marked as on a queue.
    ///
    /// # Safety
    ///
    /// `node` must point at a live node whose link is on no queue, and it
    /// must be valid for reads and writes of the whole node.  Until the
    /// node leaves the queue:
    ///
    /// - it must stay live and unmoved;
    /// - nothing but the queue may write its link, and no read of the
    ///   link through another reference, as [`Link::is_linked`] makes,
    ///   may race with the queue's writes;
    /// - no access to the node may conflict with the references the queue
    ///   hands out to it: `&A::Node`, and `&mut A::Payload` from a
    ///   [`Payload`] adapter.
    ///
    /// If the queue is dropped while it holds the node, the node must
    /// still be live then.  A node that leaves as `&'nodes mut A::Node`
    /// becomes that reference, so then `node` must have been made from a
    /// `&mut` or `*mut` to the whole node, and nothing else may access the
    /// node for as long as the reference is used.
    #[inline]
    pub unsafe fn push_front_ptr(&mut self, node: NonNull<A::Node>) {
        // SAFETY: the caller keeps the push contract.
        let link = unsafe { A::link(node) };
        // SAFETY: the node is live.
        let new = unsafe { link.as_ref() };
        debug_assert!(
            !new.is_linked(),
            "tail queue: the link is already on a queue",
        );
        let next = self.first.replace(link);
        new.next.set(next);
        new.prev.set(Some(NOWHERE));
        match next {
            // SAFETY: the old first node is live.
            Some(after) => unsafe { after.as_ref() }.prev.set(Some(link)),
            None => self.last = Some(link),
        }
        self.count.add(1);
    }

    /// Pushes the node at `node` at the back.
    ///
    /// # Panics
    ///
    /// In debug builds, when the node's link is marked as on a queue.
    ///
    /// # Safety
    ///
    /// `node` must meet the contract of
    /// [`push_front_ptr`](Self::push_front_ptr).
    #[inline]
    pub unsafe fn push_back_ptr(&mut self, node: NonNull<A::Node>) {
        // SAFETY: the caller keeps the push contract.
        let link = unsafe { A::link(node) };
        // SAFETY: the node is live.
        let new = unsafe { link.as_ref() };
        debug_assert!(
            !new.is_linked(),
            "tail queue: the link is already on a queue",
        );
        let before = self.last.replace(link);
        new.next.set(None);
        new.prev.set(Some(before.unwrap_or(NOWHERE)));
        match before {
            // SAFETY: the old last node is live.
            Some(last) => unsafe { last.as_ref() }.next.set(Some(link)),
            None => self.first = Some(link),
        }
        self.count.add(1);
    }

    /// Unlinks the first node and returns it, or `None` when empty.
    ///
    /// # Panics
    ///
    /// In debug builds, when the first node's link is not consistent with
    /// its neighbours: the queue is corrupt.
    #[must_use]
    #[inline]
    pub fn pop_front(&mut self) -> Option<&'nodes mut A::Node> {
        // SAFETY: the node has left the queue, which held it as
        // `&'nodes mut` or under the contract of a `_ptr` push.
        self.pop_front_ptr()
            .map(|mut node| unsafe { node.as_mut() })
    }

    /// Unlinks the last node and returns it, or `None` when empty.
    ///
    /// # Panics
    ///
    /// In debug builds, when the last node's link is not consistent with
    /// its neighbours: the queue is corrupt.
    #[must_use]
    #[inline]
    pub fn pop_back(&mut self) -> Option<&'nodes mut A::Node> {
        // SAFETY: the node has left the queue, which held it as
        // `&'nodes mut` or under the contract of a `_ptr` push.
        self.pop_back_ptr().map(|mut node| unsafe { node.as_mut() })
    }

    /// Unlinks the first node and returns the pointer it was pushed with,
    /// or `None` when empty.
    ///
    /// # Panics
    ///
    /// In debug builds, when the first node's link is not consistent with
    /// its neighbours: the queue is corrupt.
    #[must_use]
    #[inline]
    pub fn pop_front_ptr(&mut self) -> Option<NonNull<A::Node>> {
        let first = self.first?;
        // SAFETY: `first` is the link of a live node on this queue.
        unsafe { self.check(first) };
        // SAFETY: as above.
        let link = unsafe { first.as_ref() };
        let next = link.next.get();
        self.first = next;
        match next {
            // SAFETY: the next node is live.
            Some(after) => unsafe { after.as_ref() }.prev.set(Some(NOWHERE)),
            None => self.last = None,
        }
        self.count.sub_one();
        release(link);
        // SAFETY: the node is live.
        Some(unsafe { A::node(first) })
    }

    /// Unlinks the last node and returns the pointer it was pushed with,
    /// or `None` when empty.
    ///
    /// # Panics
    ///
    /// In debug builds, when the last node's link is not consistent with
    /// its neighbours: the queue is corrupt.
    #[must_use]
    #[inline]
    pub fn pop_back_ptr(&mut self) -> Option<NonNull<A::Node>> {
        let last = self.last?;
        // SAFETY: `last` is the link of a live node on this queue.
        unsafe { self.check(last) };
        // SAFETY: as above.
        let link = unsafe { last.as_ref() };
        // SAFETY: the link is on this queue.
        let before = unsafe { link.prev_link() };
        match before {
            // SAFETY: the previous node is live.
            Some(prev) => unsafe { prev.as_ref() }.next.set(None),
            None => self.first = None,
        }
        self.last = before;
        self.count.sub_one();
        release(link);
        // SAFETY: the node is live.
        Some(unsafe { A::node(last) })
    }

    /// Unlinks the node at `node` in O(1).  The node is not handed back:
    /// the caller already holds a pointer to it, and may push it again
    /// with a `_ptr` push, or reach it as `&'nodes mut` through
    /// [`cursor_mut_from_ptr`](Self::cursor_mut_from_ptr) and
    /// [`remove_current`](CursorMut::remove_current) instead.
    ///
    /// # Panics
    ///
    /// In debug builds, when the node's link is on no queue, when it is
    /// not consistent with its neighbours, or when it is at an end of a
    /// queue other than this one.
    ///
    /// # Safety
    ///
    /// `node` must be on this queue.
    #[inline]
    pub unsafe fn remove_ptr(&mut self, node: NonNull<A::Node>) {
        // SAFETY: the caller puts the node on this queue.
        unsafe { self.unlink(A::link(node)) };
    }

    /// Moves every node of `other` to the back of this queue in O(1),
    /// leaving `other` empty.
    #[inline]
    pub fn append(&mut self, other: &mut Self) {
        let Some(first) = other.first.take() else {
            return;
        };
        // SAFETY: `first` is the link of a live node on `other`.
        let head = unsafe { first.as_ref() };
        match self.last {
            Some(last) => {
                // SAFETY: the last node of this queue is live.
                unsafe { last.as_ref() }.next.set(Some(first));
                head.prev.set(Some(last));
            }
            None => self.first = Some(first),
        }
        self.last = other.last.take();
        self.count.add(other.count.take());
    }

    /// Returns the queue's own pointer to the link `at`: the one the head
    /// or the previous link holds, which carries the access the node was
    /// pushed with.
    ///
    /// # Panics
    ///
    /// In debug builds, when `at` is on no queue, or when it is the first
    /// link of another queue.
    ///
    /// # Safety
    ///
    /// `at` must be the link of a node on this queue.
    #[inline]
    unsafe fn linked(&self, at: NonNull<Link>) -> NonNull<Link> {
        // SAFETY: the node is live.
        let link = unsafe { at.as_ref() };
        debug_assert!(link.is_linked(), "tail queue: the link is on no queue");
        // SAFETY: the link is on a queue, and the previous node is live.
        let own = unsafe { link.prev_link() }
            .map_or(self.first, |prev| unsafe { prev.as_ref() }.next.get());
        debug_assert!(
            own == Some(at),
            "tail queue: the link is not on this queue",
        );
        own.unwrap_or(at)
    }

    /// Checks, in debug builds, that the link `at` is on a queue, that its
    /// neighbours point back at it, and that at either end it is this
    /// queue's.
    ///
    /// # Panics
    ///
    /// In debug builds, when one of those does not hold.
    ///
    /// # Safety
    ///
    /// `at` must be the link of a live node.
    #[inline]
    unsafe fn check(&self, at: NonNull<Link>) {
        #[cfg(debug_assertions)]
        {
            // SAFETY: the node is live.
            let link = unsafe { at.as_ref() };
            assert!(link.is_linked(), "tail queue: the link is on no queue");
            match link.next.get() {
                // SAFETY: a link on a queue has a live next node.
                Some(next) => assert!(
                    unsafe { next.as_ref() }.prev.get() == Some(at),
                    "tail queue: the next link does not point back",
                ),
                None => assert!(
                    self.last == Some(at),
                    "tail queue: the link is not on this queue",
                ),
            }
            // SAFETY: the link is on a queue.
            match unsafe { link.prev_link() } {
                // SAFETY: a link on a queue has a live previous node.
                Some(prev) => assert!(
                    unsafe { prev.as_ref() }.next.get() == Some(at),
                    "tail queue: the previous link does not point at this one",
                ),
                None => assert!(
                    self.first == Some(at),
                    "tail queue: the link is not on this queue",
                ),
            }
        }
        #[cfg(not(debug_assertions))]
        let _: NonNull<Link> = at;
    }

    /// Returns the link before `current`: the last at the ghost, and
    /// `None` before the first node.
    ///
    /// # Safety
    ///
    /// `current` must be `None` or the link of a live node on this queue.
    #[inline]
    unsafe fn prev_before(
        &self,
        current: Option<NonNull<Link>>,
    ) -> Option<NonNull<Link>> {
        // SAFETY: the node is live and on this queue.
        current.map_or(self.last, |link| unsafe { link.as_ref().prev_link() })
    }

    /// Returns the link after `current`: the first at the ghost, and
    /// `None` after the last node.
    ///
    /// # Safety
    ///
    /// `current` must be `None` or the link of a live node on this queue.
    #[inline]
    const unsafe fn next_after(
        &self,
        current: Option<NonNull<Link>>,
    ) -> Option<NonNull<Link>> {
        match current {
            None => self.first,
            // SAFETY: the node is live.
            Some(link) => unsafe { link.as_ref() }.next.get(),
        }
    }

    /// Links `link` after the link `at`.
    ///
    /// # Panics
    ///
    /// In debug builds, when `link` is marked as on a queue, or when `at`
    /// is not consistent with its neighbours.
    ///
    /// # Safety
    ///
    /// `at` must be the link of a node on this queue, and `link` must meet
    /// the push contract of [`push_front_ptr`](Self::push_front_ptr).
    #[inline]
    unsafe fn link_after(&mut self, at: NonNull<Link>, link: NonNull<Link>) {
        // SAFETY: `at` is the link of a live node on this queue.
        unsafe { self.check(at) };
        // SAFETY: both nodes are live.
        let (anchor, new) = unsafe { (at.as_ref(), link.as_ref()) };
        debug_assert!(
            !new.is_linked(),
            "tail queue: the link is already on a queue",
        );
        let next = anchor.next.replace(Some(link));
        new.next.set(next);
        new.prev.set(Some(at));
        match next {
            // SAFETY: the next node is live.
            Some(after) => unsafe { after.as_ref() }.prev.set(Some(link)),
            None => self.last = Some(link),
        }
        self.count.add(1);
    }

    /// Links `link` before the link `at`.
    ///
    /// # Panics
    ///
    /// In debug builds, when `link` is marked as on a queue, or when `at`
    /// is not consistent with its neighbours.
    ///
    /// # Safety
    ///
    /// `at` must be the link of a node on this queue, and `link` must meet
    /// the push contract of [`push_front_ptr`](Self::push_front_ptr).
    #[inline]
    unsafe fn link_before(&mut self, at: NonNull<Link>, link: NonNull<Link>) {
        // SAFETY: `at` is the link of a live node on this queue.
        unsafe { self.check(at) };
        // SAFETY: both nodes are live.
        let (anchor, new) = unsafe { (at.as_ref(), link.as_ref()) };
        debug_assert!(
            !new.is_linked(),
            "tail queue: the link is already on a queue",
        );
        let prev = anchor.prev.replace(Some(link));
        new.next.set(Some(at));
        new.prev.set(prev);
        // SAFETY: `at` was on this queue, so `prev` is its old `prev`.
        match unsafe { new.prev_link() } {
            // SAFETY: the previous node is live.
            Some(before) => unsafe { before.as_ref() }.next.set(Some(link)),
            None => self.first = Some(link),
        }
        self.count.add(1);
    }

    /// Unlinks the node whose link is `at`.
    ///
    /// # Panics
    ///
    /// In debug builds, when `at` is on no queue, when it is not consistent
    /// with its neighbours, or when it is at an end of another queue.
    ///
    /// # Safety
    ///
    /// `at` must be the link of a node on this queue.
    #[inline]
    unsafe fn unlink(&mut self, at: NonNull<Link>) {
        // SAFETY: `at` is the link of a live node.
        unsafe { self.check(at) };
        // SAFETY: as above.
        let link = unsafe { at.as_ref() };
        let next = link.next.get();
        // SAFETY: the link is on this queue.
        let before = unsafe { link.prev_link() };
        match before {
            // SAFETY: the previous node is live.
            Some(prev) => unsafe { prev.as_ref() }.next.set(next),
            None => self.first = next,
        }
        match next {
            // SAFETY: the next node is live.
            Some(after) => unsafe { after.as_ref() }.prev.set(link.prev.get()),
            None => self.last = before,
        }
        self.count.sub_one();
        release(link);
    }

    /// Puts `link` where the link `at` is and unlinks `at`.
    ///
    /// # Panics
    ///
    /// In debug builds, when `link` is marked as on a queue, when `at` is
    /// on no queue, or when it is not consistent with its neighbours.
    ///
    /// # Safety
    ///
    /// `at` must be the link of a node on this queue, and `link` must
    /// meet the push contract of [`push_front_ptr`](Self::push_front_ptr).
    #[inline]
    unsafe fn replace(&mut self, at: NonNull<Link>, link: NonNull<Link>) {
        // SAFETY: `at` is the link of a live node.
        unsafe { self.check(at) };
        // SAFETY: both nodes are live.
        let (old, new) = unsafe { (at.as_ref(), link.as_ref()) };
        debug_assert!(
            !new.is_linked(),
            "tail queue: the link is already on a queue",
        );
        let next = old.next.get();
        new.next.set(next);
        new.prev.set(old.prev.get());
        match next {
            // SAFETY: the next node is live.
            Some(after) => unsafe { after.as_ref() }.prev.set(Some(link)),
            None => self.last = Some(link),
        }
        // SAFETY: `at` is on this queue.
        match unsafe { old.prev_link() } {
            // SAFETY: the previous node is live.
            Some(before) => unsafe { before.as_ref() }.next.set(Some(link)),
            None => self.first = Some(link),
        }
        release(old);
    }

    /// Marks every node's link as on no queue and empties the queue.
    #[inline]
    fn release_all(&mut self) {
        let mut at = self.first.take();
        while let Some(ptr) = at {
            // SAFETY: a node stays live while it is on the queue.
            let link = unsafe { ptr.as_ref() };
            at = link.next.get();
            release(link);
        }
        self.last = None;
        let _: usize = self.count.take();
    }
}

impl<A: Payload, C: Count> TailQueue<'_, A, C> {
    /// Returns the payload of the first node, or `None` when empty.
    #[must_use]
    #[inline]
    pub fn front_mut(&mut self) -> Option<&mut A::Payload> {
        // SAFETY: a node stays live while it is on the queue, its payload
        // lies apart from its link, and the queue lends it only while
        // borrowed mutably.
        self.first
            .map(|link| unsafe { A::payload(A::node(link)).as_mut() })
    }

    /// Returns the payload of the last node, or `None` when empty.
    #[must_use]
    #[inline]
    pub fn back_mut(&mut self) -> Option<&mut A::Payload> {
        // SAFETY: as for `front_mut`.
        self.last
            .map(|link| unsafe { A::payload(A::node(link)).as_mut() })
    }

    /// Returns an iterator over the nodes' payloads, front to back;
    /// reversed, back to front.
    #[must_use]
    #[inline]
    pub const fn iter_mut(&mut self) -> IterMut<'_, A> {
        IterMut::new(IterPtr::new(self.first, self.last))
    }
}

impl<A: Adapter> TailQueue<'_, A, Counted> {
    /// Returns how many nodes the queue holds, in O(1).
    #[must_use]
    #[inline]
    pub const fn len(&self) -> usize {
        self.count.get()
    }
}

impl<A: Adapter, C: Count> Default for TailQueue<'_, A, C> {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl<'nodes, A: Adapter, C: Count> Extend<&'nodes mut A::Node>
    for TailQueue<'nodes, A, C>
{
    /// Pushes each node at the back, in order.
    ///
    /// # Panics
    ///
    /// In debug builds, when a node's link is marked as on a queue.
    #[inline]
    fn extend<I>(&mut self, iter: I)
    where
        I: IntoIterator<Item = &'nodes mut A::Node>,
    {
        for node in iter {
            self.push_back(node);
        }
    }
}

impl<'nodes, A: Adapter, C: Count> FromIterator<&'nodes mut A::Node>
    for TailQueue<'nodes, A, C>
{
    /// Returns a queue of the nodes, in order.
    ///
    /// # Panics
    ///
    /// In debug builds, when a node's link is marked as on a queue.
    #[inline]
    fn from_iter<I>(iter: I) -> Self
    where
        I: IntoIterator<Item = &'nodes mut A::Node>,
    {
        let mut queue = Self::new();
        queue.extend(iter);
        queue
    }
}

impl<A: Adapter, C: Count> Drop for TailQueue<'_, A, C> {
    /// Marks the links of the nodes still on the queue as on no queue.
    #[inline]
    fn drop(&mut self) {
        self.release_all();
    }
}

impl<'head, A: Adapter, C: Count> IntoIterator for &'head TailQueue<'_, A, C> {
    type Item = &'head A::Node;
    type IntoIter = Iter<'head, A>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'head, A: Payload, C: Count> IntoIterator
    for &'head mut TailQueue<'_, A, C>
{
    type Item = &'head mut A::Payload;
    type IntoIter = IterMut<'head, A>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}

impl<A: Adapter, C: Count> fmt::Debug for TailQueue<'_, A, C> {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TailQueue")
            .field("first", &self.first)
            .field("last", &self.last)
            .field("count", &self.count)
            .finish()
    }
}

// SAFETY: the queue holds only pointers to nodes it hands out as `&Node`
// or `&'nodes mut Node`; `Node: Send + Sync` lets that access move threads.
unsafe impl<A: Adapter, C: Count + Send> Send for TailQueue<'_, A, C> where
    A::Node: Send + Sync
{
}
// SAFETY: `&TailQueue` only reads the head and the links and yields
// `&Node`, which `Node: Sync` lets several threads hold at once.
unsafe impl<A: Adapter, C: Count> Sync for TailQueue<'_, A, C> where
    A::Node: Send + Sync
{
}

/// Marks a leaving link as on no queue.  Debug builds also fill its
/// forward word with [`NOWHERE`], so a stale walk faults on a
/// recognisable address.
#[inline]
fn release(link: &Link) {
    link.prev.set(None);
    #[cfg(debug_assertions)]
    link.next.set(Some(NOWHERE));
}
