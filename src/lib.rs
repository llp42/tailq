// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Leonardo Lopes Pereira <leonardolopespereira@outlook.com>

//! An intrusive tail queue: a two-word head and two-word links, walked
//! both ways, whose links live inside caller-owned nodes, so the queue
//! never allocates or frees.  The crate is `no_std` and needs no `alloc`.
//!
//! ```
//! use core::pin::pin;
//! use tailq::Link;
//!
//! struct Thread {
//!     id: u32,
//!     link: Link,
//! }
//!
//! tailq::queue!(RunQueue<'nodes> = Thread { link });
//!
//! let mut first = Thread { id: 1, link: Link::new() };
//! let mut second = Thread { id: 2, link: Link::new() };
//! let mut run_queue = pin!(RunQueue::new());
//! run_queue.as_mut().push_back(&mut first);
//! run_queue.as_mut().push_front(&mut second);
//!
//! let ids: Vec<u32> = run_queue.iter().map(|thread| thread.id).collect();
//! assert_eq!(ids, [2, 1]);
//!
//! let next = run_queue.as_mut().pop_front();
//! assert!(next.is_some_and(|thread| thread.id == 2 && !thread.link.is_linked()));
//! ```
//!
//! [`queue!`] declares a queue type for a node type and the field that
//! links it; one node joins several queues at once through several
//! fields.
//!
//! The head embeds a link of its own, `ends`: its `next` is the first
//! node's link and its `prev` the last node's.  Each node's link holds
//! the next node's link and the link whose `next` points back at it:
//! the previous node's, or `ends` for the first node.  So every back
//! pointer names a link, a backward step is one load and one compare
//! with `ends`, and any node leaves in O(1) given the head.  Pushing at
//! either end, linking before or after a cursor, and appending another
//! queue are O(1) too.
//!
//! The first node's link points into the head, so a head is pinned: it
//! mutates through `Pin<&mut Self>` and stays in place while it holds
//! nodes.  A head on the stack is pinned with [`core::pin::pin!`].  A head
//! in a `static` never moves, so one reached through a lock guard on that
//! static is pinned with [`Pin::new_unchecked`].  A head of all zero bytes
//! is an empty queue, so heads inside zeroed memory need no
//! initialisation.
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
//! check that a link's neighbours point back at it before every operation
//! on it and that a pushed link is on no queue, and fill a leaving link's
//! forward word with a dangling address.

#![no_std]

mod adapter;
mod count;
mod cursor;

pub use adapter::{Adapter, Field, LinkAt};
pub use count::{Count, Counted, Uncounted};
pub use cursor::{Cursor, CursorMut, Iter, IterPtr};

use core::cell::Cell;
use core::fmt;
use core::marker::{PhantomData, PhantomPinned};
use core::mem::size_of;
use core::pin::Pin;
use core::ptr::{self, NonNull};

/// The field a node embeds to join one [`TailQueue`].
///
/// Two words: the next node's link, and the link whose `next` points at
/// this one, null while the link is on no queue.  A link of all zero
/// bytes is the one [`new`](Self::new) returns.
#[repr(C)]
#[derive(Debug)]
pub struct Link {
    next: Cell<Option<NonNull<Self>>>,
    /// The previous node's link, or the head's `ends` for the first node;
    /// null while on no queue.
    prev: Cell<*const Self>,
}

impl Link {
    /// Returns a link for a node that is on no queue.
    #[must_use]
    #[inline]
    pub const fn new() -> Self {
        Self {
            next: Cell::new(None),
            prev: Cell::new(ptr::null()),
        }
    }

    /// Returns whether the link is on a queue.
    ///
    /// A link left on a queue that was forgotten, or overwritten without
    /// being dropped, still reads as linked.
    #[must_use]
    #[inline]
    pub const fn is_linked(&self) -> bool {
        !self.prev.get().is_null()
    }
}

impl Default for Link {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

// SAFETY: a link's words are private to this crate and only reached
// through the queue holding its node, which reads them under `&` and
// writes them under `Pin<&mut>`; the queue's borrow orders every access.
unsafe impl Send for Link {}
// SAFETY: shared access to a link reaches no word of it but the mark
// `is_linked` reads; its queue reads under `&` and writes only under
// `Pin<&mut>`.
unsafe impl Sync for Link {}

const _: () = assert!(
    size_of::<Link>() == 2 * size_of::<usize>(),
    "a link is two words",
);

/// The head of a tail queue of `A::Node`s borrowed for `'nodes`, which
/// keeps its length when `C` is [`Counted`].
///
/// A head of all zero bytes is an empty queue.  Dropping a head marks
/// the links of the nodes still on it as on no queue, so those nodes must
/// be live then.
pub struct TailQueue<'nodes, A: Adapter, C: Count = Uncounted> {
    /// `ends.next`: the first node's link.  `ends.prev`: the link whose
    /// `next` ends the queue, the last node's or, when empty, `ends`
    /// itself; null stands for `ends`, so that [`new`](Self::new) needs
    /// no address.
    ends: Link,
    count: C,
    nodes: PhantomData<(A, &'nodes mut A::Node)>,
    _pinned: PhantomPinned,
}

impl<'nodes, A: Adapter, C: Count> TailQueue<'nodes, A, C> {
    /// Returns an empty queue.
    #[must_use]
    #[inline]
    pub const fn new() -> Self {
        Self {
            ends: Link::new(),
            count: C::ZERO,
            nodes: PhantomData,
            _pinned: PhantomPinned,
        }
    }

    /// Returns whether the queue holds no nodes.
    #[must_use]
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.ends.next.get().is_none()
    }

    /// Empties the queue in O(n), marking every node's link as on no
    /// queue.  The nodes pushed as `&'nodes mut` stay borrowed until
    /// `'nodes` ends.
    #[inline]
    pub fn clear(self: Pin<&mut Self>) {
        self.into_ref().get_ref().release_all();
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
    /// empty.  It names the node only while the node is on the queue, and
    /// its link must never be written through it.
    #[must_use]
    #[inline]
    pub fn front_ptr(&self) -> Option<NonNull<A::Node>> {
        // SAFETY: a node stays live while it is on the queue.
        self.ends.next.get().map(|link| unsafe { A::node(link) })
    }

    /// Returns the pointer the last node was pushed with, or `None` when
    /// empty, as [`front_ptr`](Self::front_ptr) does.
    #[must_use]
    #[inline]
    pub fn back_ptr(&self) -> Option<NonNull<A::Node>> {
        // SAFETY: a node stays live while it is on the queue.
        self.last().map(|link| unsafe { A::node(link) })
    }

    /// Returns an iterator over the nodes, front to back; reversed, back
    /// to front.
    #[must_use]
    #[inline]
    pub fn iter(&self) -> Iter<'_, A> {
        Iter::new(self.iter_ptr())
    }

    /// Returns an iterator over the pointers the nodes were pushed with,
    /// front to back; reversed, back to front.  Each names its node only
    /// while the node is on the queue, and its link must never be written
    /// through it.
    #[must_use]
    #[inline]
    pub fn iter_ptr(&self) -> IterPtr<'_, A> {
        IterPtr::new(self.ends.next.get(), self.last())
    }

    /// Returns a cursor at the first node, or at the ghost when empty.
    #[must_use]
    #[inline]
    pub const fn cursor_front(&self) -> Cursor<'_, 'nodes, A, C> {
        Cursor::new(self, self.ends.next.get())
    }

    /// Returns a cursor at the last node, or at the ghost when empty.
    #[must_use]
    #[inline]
    pub fn cursor_back(&self) -> Cursor<'_, 'nodes, A, C> {
        Cursor::new(self, self.last())
    }

    /// Returns a cursor at `node`.
    ///
    /// # Panics
    ///
    /// In debug builds, when the node's link is on no queue.
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
        Cursor::new(self, Some(unsafe { linked(A::link(node)) }))
    }

    /// Returns a mutable cursor at the first node, or at the ghost when
    /// empty.
    #[must_use]
    #[inline]
    pub const fn cursor_front_mut(
        self: Pin<&mut Self>,
    ) -> CursorMut<'_, 'nodes, A, C> {
        let this = self.into_ref().get_ref();
        CursorMut::new(this, this.ends.next.get())
    }

    /// Returns a mutable cursor at the last node, or at the ghost when
    /// empty.
    #[must_use]
    #[inline]
    pub fn cursor_back_mut(
        self: Pin<&mut Self>,
    ) -> CursorMut<'_, 'nodes, A, C> {
        let this = self.into_ref().get_ref();
        CursorMut::new(this, this.last())
    }

    /// Returns a mutable cursor at `node`.
    ///
    /// # Panics
    ///
    /// In debug builds, when the node's link is on no queue.
    ///
    /// # Safety
    ///
    /// `node` must be on this queue.
    #[must_use]
    #[inline]
    pub unsafe fn cursor_mut_from_ptr(
        self: Pin<&mut Self>,
        node: NonNull<A::Node>,
    ) -> CursorMut<'_, 'nodes, A, C> {
        let current = Some(unsafe { linked(A::link(node)) });
        CursorMut::new(self.into_ref().get_ref(), current)
    }

    /// Pushes `node` at the front.
    ///
    /// # Panics
    ///
    /// In debug builds, when the node's link is marked as on a queue, or
    /// when the first node's link does not point back at the head: the
    /// queue is corrupt.
    #[inline]
    pub fn push_front(self: Pin<&mut Self>, node: &'nodes mut A::Node) {
        // SAFETY: the `'nodes` borrow keeps the node live, unmoved and
        // reached only through the queue, and a `&mut` is on no other
        // queue.
        unsafe { self.push_front_ptr(NonNull::from(node)) };
    }

    /// Pushes `node` at the back.
    ///
    /// # Panics
    ///
    /// In debug builds, when the node's link is marked as on a queue, or
    /// when the tail link has a successor: the queue is corrupt.
    #[inline]
    pub fn push_back(self: Pin<&mut Self>, node: &'nodes mut A::Node) {
        // SAFETY: the `'nodes` borrow keeps the node live, unmoved and
        // reached only through the queue, and a `&mut` is on no other
        // queue.
        unsafe { self.push_back_ptr(NonNull::from(node)) };
    }

    /// Pushes the node at `node` at the front.
    ///
    /// # Panics
    ///
    /// In debug builds, when the node's link is marked as on a queue, or
    /// when the first node's link does not point back at the head: the
    /// queue is corrupt.
    ///
    /// # Safety
    ///
    /// `node` must point at a live node whose link is on no queue.  Until
    /// the node leaves the queue, it must stay live and unmoved, nothing
    /// but the queue may touch its link, and no access to the rest of it
    /// may conflict with the references the queue hands out; if the
    /// queue is dropped while it holds the node, the node must still be
    /// live then.  A node that leaves as `&'nodes mut` must stay live for
    /// as long as that is used.
    #[inline]
    pub unsafe fn push_front_ptr(self: Pin<&mut Self>, node: NonNull<A::Node>) {
        let this = self.into_ref().get_ref();
        unsafe { this.link_after(this.ends_link(), A::link(node)) };
    }

    /// Pushes the node at `node` at the back.
    ///
    /// # Panics
    ///
    /// In debug builds, when the node's link is marked as on a queue, or
    /// when the tail link has a successor: the queue is corrupt.
    ///
    /// # Safety
    ///
    /// `node` must meet the contract of
    /// [`push_front_ptr`](Self::push_front_ptr).
    #[inline]
    pub unsafe fn push_back_ptr(self: Pin<&mut Self>, node: NonNull<A::Node>) {
        let this = self.into_ref().get_ref();
        let tail = this.tail();
        debug_assert!(
            unsafe { tail.as_ref() }.next.get().is_none(),
            "tail queue: the tail link has a successor",
        );
        unsafe { this.link_after(tail, A::link(node)) };
    }

    /// Unlinks the first node and returns it, or `None` when empty.
    ///
    /// # Panics
    ///
    /// In debug builds, when a neighbour of the first node's link does
    /// not point back at it: the queue is corrupt.
    #[must_use]
    #[inline]
    pub fn pop_front(self: Pin<&mut Self>) -> Option<&'nodes mut A::Node> {
        // SAFETY: the node has left the queue, which held it as
        // `&'nodes mut` or under the contract of a `_ptr` push.
        self.pop_front_ptr()
            .map(|mut node| unsafe { node.as_mut() })
    }

    /// Unlinks the last node and returns it, or `None` when empty.
    ///
    /// # Panics
    ///
    /// In debug builds, when a neighbour of the last node's link does not
    /// point back at it: the queue is corrupt.
    #[must_use]
    #[inline]
    pub fn pop_back(self: Pin<&mut Self>) -> Option<&'nodes mut A::Node> {
        // SAFETY: the node has left the queue, which held it as
        // `&'nodes mut` or under the contract of a `_ptr` push.
        self.pop_back_ptr().map(|mut node| unsafe { node.as_mut() })
    }

    /// Unlinks the first node and returns the pointer it was pushed with,
    /// or `None` when empty.
    ///
    /// # Panics
    ///
    /// In debug builds, when a neighbour of the first node's link does
    /// not point back at it: the queue is corrupt.
    #[must_use]
    #[inline]
    pub fn pop_front_ptr(self: Pin<&mut Self>) -> Option<NonNull<A::Node>> {
        let this = self.into_ref().get_ref();
        let first = this.ends.next.get()?;
        // SAFETY: `first` is the link of a live node on this queue.
        unsafe {
            this.unlink(first);
            Some(A::node(first))
        }
    }

    /// Unlinks the last node and returns the pointer it was pushed with,
    /// or `None` when empty.
    ///
    /// # Panics
    ///
    /// In debug builds, when a neighbour of the last node's link does not
    /// point back at it: the queue is corrupt.
    #[must_use]
    #[inline]
    pub fn pop_back_ptr(self: Pin<&mut Self>) -> Option<NonNull<A::Node>> {
        let this = self.into_ref().get_ref();
        let last = this.last()?;
        // SAFETY: `last` is the link of a live node on this queue.
        unsafe {
            this.unlink(last);
            Some(A::node(last))
        }
    }

    /// Unlinks the node at `node` in O(1).  The node is not handed back:
    /// the caller already holds a pointer to it, and may push it again
    /// with a `_ptr` push.
    ///
    /// # Panics
    ///
    /// In debug builds, when the node's link is on no queue, or when a
    /// neighbour of it does not point back at it: the queue is corrupt.
    ///
    /// # Safety
    ///
    /// `node` must be on this queue.
    #[inline]
    pub unsafe fn remove_ptr(self: Pin<&mut Self>, node: NonNull<A::Node>) {
        let this = self.into_ref().get_ref();
        unsafe { this.unlink(A::link(node)) };
    }

    /// Moves every node of `other` to the back of this queue in O(1),
    /// leaving `other` empty.
    #[inline]
    pub fn append(self: Pin<&mut Self>, other: Pin<&mut Self>) {
        let this = self.into_ref().get_ref();
        let donor = other.into_ref().get_ref();
        let Some(first) = donor.ends.next.get() else {
            return;
        };
        let tail = this.tail();
        // SAFETY: the tail is `ends` or a live node's link, and `first`
        // is the link of a live node on `other`.
        unsafe {
            tail.as_ref().next.set(Some(first));
            first.as_ref().prev.set(tail.as_ptr());
        }
        this.ends.prev.set(donor.tail().as_ptr());
        this.count.add(donor.count.take());
        donor.ends.next.set(None);
        donor.ends.prev.set(ptr::null());
    }

    /// Returns the head's own link.
    fn ends_link(&self) -> NonNull<Link> {
        NonNull::from(&self.ends)
    }

    /// Returns the link whose `next` ends the queue: the last node's or
    /// `ends`.
    fn tail(&self) -> NonNull<Link> {
        NonNull::new(self.ends.prev.get().cast_mut())
            .unwrap_or_else(|| self.ends_link())
    }

    /// Returns the last node's link, or `None` when empty.
    fn last(&self) -> Option<NonNull<Link>> {
        self.ends.next.get().map(|_| self.tail())
    }

    /// Returns the link before `current`: the last at the ghost, and
    /// `None` before the first node.
    ///
    /// # Safety
    ///
    /// `current` must be `None` or the link of a live node on this
    /// queue.
    unsafe fn prev_before(
        &self,
        current: Option<NonNull<Link>>,
    ) -> Option<NonNull<Link>> {
        let prev = current.map_or_else(
            || self.tail(),
            |link| unsafe {
                NonNull::new_unchecked(link.as_ref().prev.get().cast_mut())
            },
        );
        (prev != self.ends_link()).then_some(prev)
    }

    /// Links `link` after the link `at`, which may be `ends`.
    ///
    /// # Panics
    ///
    /// In debug builds, when `link` is marked as on a queue, or when the
    /// link after `at` does not point back at it.
    ///
    /// # Safety
    ///
    /// `at` must be `ends` or the link of a node on this queue, and
    /// `link` must meet the push contract of
    /// [`push_front_ptr`](Self::push_front_ptr).
    unsafe fn link_after(&self, at: NonNull<Link>, link: NonNull<Link>) {
        let (anchor, new) = unsafe { (at.as_ref(), link.as_ref()) };
        debug_assert!(
            !new.is_linked(),
            "tail queue: the link is already on a queue",
        );
        let next = anchor.next.get();
        new.next.set(next);
        new.prev.set(at.as_ptr());
        match next {
            Some(after_ptr) => {
                let after = unsafe { after_ptr.as_ref() };
                debug_assert!(
                    ptr::eq(after.prev.get(), at.as_ptr()),
                    "tail queue: the next link does not point back",
                );
                after.prev.set(link.as_ptr());
            }
            None => self.ends.prev.set(link.as_ptr()),
        }
        anchor.next.set(Some(link));
        self.count.add(1);
    }

    /// Unlinks the node whose link is `at`.
    ///
    /// # Panics
    ///
    /// In debug builds, when `at` is on no queue, when a neighbour of it
    /// does not point back at it, or when it ends the queue without being
    /// its tail.
    ///
    /// # Safety
    ///
    /// `at` must be the link of a node on this queue.
    unsafe fn unlink(&self, at: NonNull<Link>) {
        unsafe { check(at) };
        let link = unsafe { at.as_ref() };
        let next = link.next.get();
        let prev = link.prev.get();
        unsafe { (*prev).next.set(next) };
        if let Some(after) = next {
            unsafe { after.as_ref() }.prev.set(prev);
        } else {
            debug_assert!(
                self.tail() == at,
                "tail queue: the last link is not the tail",
            );
            self.ends.prev.set(prev);
        }
        self.count.sub_one();
        release(link);
    }

    /// Puts `link` where the link `at` is and unlinks `at`.
    ///
    /// # Panics
    ///
    /// In debug builds, when `link` is marked as on a queue, when `at` is
    /// on no queue, or when a neighbour of `at` does not point back at it.
    ///
    /// # Safety
    ///
    /// `at` must be the link of a node on this queue, and `link` must
    /// meet the push contract of [`push_front_ptr`](Self::push_front_ptr).
    unsafe fn replace(&self, at: NonNull<Link>, link: NonNull<Link>) {
        unsafe { check(at) };
        let (old, new) = unsafe { (at.as_ref(), link.as_ref()) };
        debug_assert!(
            !new.is_linked(),
            "tail queue: the link is already on a queue",
        );
        let next = old.next.get();
        let prev = old.prev.get();
        new.next.set(next);
        new.prev.set(prev);
        match next {
            Some(after) => unsafe { after.as_ref() }.prev.set(link.as_ptr()),
            None => self.ends.prev.set(link.as_ptr()),
        }
        unsafe { (*prev).next.set(Some(link)) };
        release(old);
    }

    /// Marks every node's link as on no queue and empties the queue.
    fn release_all(&self) {
        let mut at = self.ends.next.get();
        while let Some(ptr) = at {
            // SAFETY: a node stays live while it is on the queue.
            let link = unsafe { ptr.as_ref() };
            at = link.next.get();
            release(link);
        }
        self.ends.next.set(None);
        self.ends.prev.set(ptr::null());
        let _: usize = self.count.take();
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

impl<A: Adapter, C: Count> fmt::Debug for TailQueue<'_, A, C> {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TailQueue")
            .field("first", &self.ends.next.get())
            .field("last", &self.ends.prev.get())
            .field("count", &self.count)
            .finish()
    }
}

// SAFETY: the queue holds only pointers to nodes it hands out as `&Node`
// or `&'nodes mut Node`; `Node: Send + Sync` lets that access move threads.
// Its length, like its links, is written only under `Pin<&mut>`.
unsafe impl<A: Adapter, C: Count + Send> Send for TailQueue<'_, A, C> where
    A::Node: Send + Sync
{
}
// SAFETY: `&TailQueue` only reads links and the length and yields
// `&Node`, which `Node: Sync` lets several threads hold at once.
unsafe impl<A: Adapter, C: Count> Sync for TailQueue<'_, A, C> where
    A::Node: Send + Sync
{
}

/// Returns the queue's own pointer to the link `at`: the one the link
/// before it holds, which carries the access the node was pushed with.
///
/// # Panics
///
/// In debug builds, when `at` is on no queue.
///
/// # Safety
///
/// `at` must be the link of a node on a queue.
unsafe fn linked(at: NonNull<Link>) -> NonNull<Link> {
    let link = unsafe { at.as_ref() };
    debug_assert!(link.is_linked(), "tail queue: the link is on no queue");
    unsafe { (*link.prev.get()).next.get() }.unwrap_or(at)
}

/// Checks, in debug builds, that the link `at` is on a queue and that its
/// neighbours point back at it.
///
/// # Panics
///
/// In debug builds, when `at` is on no queue, when the next link's `prev`
/// is not `at`, or when the link before `at` does not point at it.
///
/// # Safety
///
/// `at` must be the link of a node on a queue.
unsafe fn check(at: NonNull<Link>) {
    let link = unsafe { at.as_ref() };
    debug_assert!(link.is_linked(), "tail queue: the link is on no queue");
    if let Some(next) = link.next.get() {
        debug_assert!(
            ptr::eq(unsafe { next.as_ref() }.prev.get(), at.as_ptr()),
            "tail queue: the next link does not point back",
        );
    }
    debug_assert!(
        unsafe { (*link.prev.get()).next.get() } == Some(at),
        "tail queue: the link before a link does not point at it",
    );
}

/// Marks a leaving link as on no queue.  Debug builds also fill its
/// forward word with a dangling address, so a stale walk faults on a
/// recognisable one.
fn release(link: &Link) {
    link.prev.set(ptr::null());
    #[cfg(debug_assertions)]
    link.next.set(Some(NonNull::dangling()));
}
