// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Leonardo Lopes Pereira <leonardolopespereira@outlook.com>

//! Cursors and the double-ended iterators over a [`TailQueue`].
//!
//! A cursor rests on a node or on the ghost, the position before the
//! front and after the back: moving forward from the last node, or back
//! from the first, reaches the ghost, and moving on from the ghost wraps
//! to the other end.

use super::{Adapter, Count, Link, Payload, TailQueue, Uncounted};
use core::fmt;
use core::marker::PhantomData;
use core::ptr::NonNull;

/// A cursor over a shared [`TailQueue`], borrowed for `'head`.
pub struct Cursor<'head, 'nodes, A: Adapter, C: Count = Uncounted> {
    queue: &'head TailQueue<'nodes, A, C>,
    current: Option<NonNull<Link>>,
}

impl<'head, 'nodes, A: Adapter, C: Count> Cursor<'head, 'nodes, A, C> {
    pub(super) const fn new(
        queue: &'head TailQueue<'nodes, A, C>,
        current: Option<NonNull<Link>>,
    ) -> Self {
        Self { queue, current }
    }

    /// Returns the node at the cursor, or `None` at the ghost.
    #[must_use]
    #[inline]
    pub fn current(&self) -> Option<&'head A::Node> {
        // SAFETY: a node stays live while it is on the queue.
        self.current_ptr().map(|node| unsafe { node.as_ref() })
    }

    /// Returns the pointer the node at the cursor was pushed with, or
    /// `None` at the ghost, as [`TailQueue::front_ptr`] does.
    #[must_use]
    #[inline]
    pub fn current_ptr(&self) -> Option<NonNull<A::Node>> {
        // SAFETY: a node stays live while it is on the queue.
        self.current.map(|link| unsafe { A::node(link) })
    }

    /// Moves to the next node: from the last node to the ghost, and from
    /// the ghost to the front.
    #[inline]
    pub const fn move_next(&mut self) {
        // SAFETY: the cursor rests on the queue's ghost or on one of its
        // nodes.
        self.current = unsafe { self.queue.next_after(self.current) };
    }

    /// Moves to the previous node: from the first node to the ghost, and
    /// from the ghost to the back.
    #[inline]
    pub fn move_prev(&mut self) {
        // SAFETY: the cursor rests on the queue's ghost or on one of its
        // nodes.
        self.current = unsafe { self.queue.prev_before(self.current) };
    }

    /// Returns the node after the cursor without moving, or `None` past
    /// the last node; from the ghost, the front.
    #[must_use]
    #[inline]
    pub fn peek_next(&self) -> Option<&'head A::Node> {
        // SAFETY: a node stays live while it is on the queue.
        self.peek_next_ptr().map(|node| unsafe { node.as_ref() })
    }

    /// Returns the node before the cursor without moving, or `None`
    /// before the first node; from the ghost, the back.
    #[must_use]
    #[inline]
    pub fn peek_prev(&self) -> Option<&'head A::Node> {
        // SAFETY: a node stays live while it is on the queue.
        self.peek_prev_ptr().map(|node| unsafe { node.as_ref() })
    }

    /// Returns a pointer to the node after the cursor, as
    /// [`peek_next`](Self::peek_next) and [`current_ptr`](Self::current_ptr)
    /// do.
    #[must_use]
    #[inline]
    pub fn peek_next_ptr(&self) -> Option<NonNull<A::Node>> {
        // SAFETY: the cursor rests on the queue's ghost or on one of its
        // nodes, and a node stays live while it is on the queue.
        unsafe { self.queue.next_after(self.current) }
            .map(|link| unsafe { A::node(link) })
    }

    /// Returns a pointer to the node before the cursor, as
    /// [`peek_prev`](Self::peek_prev) and [`current_ptr`](Self::current_ptr)
    /// do.
    #[must_use]
    #[inline]
    pub fn peek_prev_ptr(&self) -> Option<NonNull<A::Node>> {
        // SAFETY: the cursor rests on the queue's ghost or on one of its
        // nodes, and a node stays live while it is on the queue.
        unsafe { self.queue.prev_before(self.current) }
            .map(|link| unsafe { A::node(link) })
    }
}

/// A cursor over an exclusively borrowed [`TailQueue`] that can link and
/// unlink nodes.
pub struct CursorMut<'head, 'nodes, A: Adapter, C: Count = Uncounted> {
    queue: &'head mut TailQueue<'nodes, A, C>,
    current: Option<NonNull<Link>>,
}

impl<'head, 'nodes, A: Adapter, C: Count> CursorMut<'head, 'nodes, A, C> {
    pub(super) const fn new(
        queue: &'head mut TailQueue<'nodes, A, C>,
        current: Option<NonNull<Link>>,
    ) -> Self {
        Self { queue, current }
    }

    /// Returns a read-only cursor at the same position, borrowed from this
    /// one.
    #[must_use]
    #[inline]
    pub const fn as_cursor(&self) -> Cursor<'_, 'nodes, A, C> {
        Cursor::new(self.queue, self.current)
    }

    /// Returns the node at the cursor, or `None` at the ghost.
    #[must_use]
    #[inline]
    pub fn current(&self) -> Option<&A::Node> {
        self.as_cursor().current()
    }

    /// Returns the pointer the node at the cursor was pushed with, or
    /// `None` at the ghost, as [`TailQueue::front_ptr`] does.
    #[must_use]
    #[inline]
    pub fn current_ptr(&self) -> Option<NonNull<A::Node>> {
        self.as_cursor().current_ptr()
    }

    /// Moves to the next node: from the last node to the ghost, and from
    /// the ghost to the front.
    #[inline]
    pub const fn move_next(&mut self) {
        let mut cursor = self.as_cursor();
        cursor.move_next();
        self.current = cursor.current;
    }

    /// Moves to the previous node: from the first node to the ghost, and
    /// from the ghost to the back.
    #[inline]
    pub fn move_prev(&mut self) {
        let mut cursor = self.as_cursor();
        cursor.move_prev();
        self.current = cursor.current;
    }

    /// Returns the node after the cursor without moving, or `None` past
    /// the last node; from the ghost, the front.
    #[must_use]
    #[inline]
    pub fn peek_next(&self) -> Option<&A::Node> {
        self.as_cursor().peek_next()
    }

    /// Returns the node before the cursor without moving, or `None`
    /// before the first node; from the ghost, the back.
    #[must_use]
    #[inline]
    pub fn peek_prev(&self) -> Option<&A::Node> {
        self.as_cursor().peek_prev()
    }

    /// Returns a pointer to the node after the cursor, as
    /// [`peek_next`](Self::peek_next) and [`current_ptr`](Self::current_ptr)
    /// do.
    #[must_use]
    #[inline]
    pub fn peek_next_ptr(&self) -> Option<NonNull<A::Node>> {
        self.as_cursor().peek_next_ptr()
    }

    /// Returns a pointer to the node before the cursor, as
    /// [`peek_prev`](Self::peek_prev) and [`current_ptr`](Self::current_ptr)
    /// do.
    #[must_use]
    #[inline]
    pub fn peek_prev_ptr(&self) -> Option<NonNull<A::Node>> {
        self.as_cursor().peek_prev_ptr()
    }

    /// Links `node` after the cursor; at the ghost, at the front.  The
    /// cursor does not move.
    ///
    /// # Panics
    ///
    /// In debug builds, when the node's link is marked as on a queue, or
    /// when the cursor's link is not consistent with its neighbours: the
    /// queue is corrupt.
    #[inline]
    pub fn insert_after(&mut self, node: &'nodes mut A::Node) {
        // SAFETY: the `'nodes` borrow keeps the node live, unmoved and
        // reached only through the queue, a `&mut` allows writes to all
        // of it, and it is on no other queue.
        unsafe { self.insert_after_ptr(NonNull::from(node)) };
    }

    /// Links the node at `node` after the cursor; at the ghost, at the
    /// front.  The cursor does not move.
    ///
    /// # Panics
    ///
    /// In debug builds, when the node's link is marked as on a queue, or
    /// when the cursor's link is not consistent with its neighbours: the
    /// queue is corrupt.
    ///
    /// # Safety
    ///
    /// `node` must meet the contract of
    /// [`push_front_ptr`](TailQueue::push_front_ptr).
    #[inline]
    pub unsafe fn insert_after_ptr(&mut self, node: NonNull<A::Node>) {
        match self.current {
            // SAFETY: the caller keeps the push contract.
            None => unsafe { self.queue.push_front_ptr(node) },
            // SAFETY: the cursor rests on a node of the queue it borrows
            // exclusively, and the caller keeps the push contract.
            Some(at) => unsafe { self.queue.link_after(at, A::link(node)) },
        }
    }

    /// Links `node` before the cursor; at the ghost, at the back.  The
    /// cursor does not move.
    ///
    /// # Panics
    ///
    /// In debug builds, when the node's link is marked as on a queue, or
    /// when the cursor's link is not consistent with its neighbours: the
    /// queue is corrupt.
    #[inline]
    pub fn insert_before(&mut self, node: &'nodes mut A::Node) {
        // SAFETY: the `'nodes` borrow keeps the node live, unmoved and
        // reached only through the queue, a `&mut` allows writes to all
        // of it, and it is on no other queue.
        unsafe { self.insert_before_ptr(NonNull::from(node)) };
    }

    /// Links the node at `node` before the cursor; at the ghost, at the
    /// back.  The cursor does not move.
    ///
    /// # Panics
    ///
    /// In debug builds, when the node's link is marked as on a queue, or
    /// when the cursor's link is not consistent with its neighbours: the
    /// queue is corrupt.
    ///
    /// # Safety
    ///
    /// `node` must meet the contract of
    /// [`push_front_ptr`](TailQueue::push_front_ptr).
    #[inline]
    pub unsafe fn insert_before_ptr(&mut self, node: NonNull<A::Node>) {
        match self.current {
            // SAFETY: the caller keeps the push contract.
            None => unsafe { self.queue.push_back_ptr(node) },
            // SAFETY: the cursor rests on a node of the queue it borrows
            // exclusively, and the caller keeps the push contract.
            Some(at) => unsafe { self.queue.link_before(at, A::link(node)) },
        }
    }

    /// Unlinks the node at the cursor and returns it, moving to the next
    /// node; `None` at the ghost.
    ///
    /// # Panics
    ///
    /// In debug builds, when the cursor's link is not consistent with its
    /// neighbours: the queue is corrupt.
    #[inline]
    pub fn remove_current(&mut self) -> Option<&'nodes mut A::Node> {
        // SAFETY: the node has left the queue, which held it as
        // `&'nodes mut` or under the contract of a `_ptr` push.
        self.remove_current_ptr()
            .map(|mut node| unsafe { node.as_mut() })
    }

    /// Unlinks the node at the cursor and returns the pointer it was
    /// pushed with, moving to the next node; `None` at the ghost.
    ///
    /// # Panics
    ///
    /// In debug builds, when the cursor's link is not consistent with its
    /// neighbours: the queue is corrupt.
    #[inline]
    pub fn remove_current_ptr(&mut self) -> Option<NonNull<A::Node>> {
        let at = self.current?;
        // SAFETY: the cursor rests on a node of the queue it borrows
        // exclusively, and that node is live.
        unsafe {
            let next = at.as_ref().next.get();
            self.queue.unlink(at);
            self.current = next;
            Some(A::node(at))
        }
    }

    /// Puts `node` where the node at the cursor is, rests on `node` and
    /// returns the node it replaced.
    ///
    /// # Panics
    ///
    /// In debug builds, when the node's link is marked as on a queue, or
    /// when the cursor's link is not consistent with its neighbours: the
    /// queue is corrupt.
    ///
    /// # Errors
    ///
    /// At the ghost, which holds no node to replace: links nothing and
    /// hands `node` back.
    #[inline]
    pub fn replace_current(
        &mut self,
        node: &'nodes mut A::Node,
    ) -> Result<&'nodes mut A::Node, &'nodes mut A::Node> {
        let Some(at) = self.current else {
            return Err(node);
        };
        // SAFETY: the `'nodes` borrow keeps the node live, unmoved and
        // reached only through the queue, a `&mut` allows writes to all
        // of it, and it is on no other queue; the cursor rests on a node
        // of the queue it borrows exclusively, and that node has left the
        // queue, which held it as `&'nodes mut` or under the contract of
        // a `_ptr` push.
        unsafe {
            let link = A::link(NonNull::from(node));
            self.queue.replace(at, link);
            self.current = Some(link);
            Ok(A::node(at).as_mut())
        }
    }

    /// Puts the node at `node` where the node at the cursor is, rests on
    /// it and returns the pointer the node it replaced was pushed with; at
    /// the ghost, links nothing and returns `None`.
    ///
    /// # Panics
    ///
    /// In debug builds, when the node's link is marked as on a queue, or
    /// when the cursor's link is not consistent with its neighbours: the
    /// queue is corrupt.
    ///
    /// # Safety
    ///
    /// `node` must meet the contract of
    /// [`push_front_ptr`](TailQueue::push_front_ptr).
    #[inline]
    pub unsafe fn replace_current_ptr(
        &mut self,
        node: NonNull<A::Node>,
    ) -> Option<NonNull<A::Node>> {
        let at = self.current?;
        // SAFETY: the caller keeps the push contract, and the cursor
        // rests on a node of the queue it borrows exclusively.
        unsafe {
            let link = A::link(node);
            self.queue.replace(at, link);
            self.current = Some(link);
            Some(A::node(at))
        }
    }
}

impl<A: Payload, C: Count> CursorMut<'_, '_, A, C> {
    /// Returns the payload of the node at the cursor, or `None` at the
    /// ghost.
    #[must_use]
    #[inline]
    pub fn current_mut(&mut self) -> Option<&mut A::Payload> {
        // SAFETY: a node stays live while it is on the queue, its payload
        // lies apart from its link, and the cursor lends it only while
        // borrowed mutably from the queue it borrows exclusively.
        self.current
            .map(|link| unsafe { A::payload(A::node(link)).as_mut() })
    }
}

/// An iterator over the pointers the nodes of a [`TailQueue`] were
/// pushed with, front to back, or back to front reversed.
pub struct IterPtr<'head, A: Adapter> {
    front: Option<NonNull<Link>>,
    back: Option<NonNull<Link>>,
    nodes: PhantomData<&'head A::Node>,
}

impl<A: Adapter> IterPtr<'_, A> {
    pub(super) const fn new(
        front: Option<NonNull<Link>>,
        back: Option<NonNull<Link>>,
    ) -> Self {
        Self {
            front,
            back,
            nodes: PhantomData,
        }
    }

    /// Ends the walk if the two ends have met on the last node left,
    /// and returns whether they had.
    #[inline]
    fn finish(&mut self) -> bool {
        let met = self.front == self.back;
        if met {
            self.front = None;
            self.back = None;
        }
        met
    }
}

impl<A: Adapter> Iterator for IterPtr<'_, A> {
    type Item = NonNull<A::Node>;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        let at = self.front?;
        if !self.finish() {
            // SAFETY: a node stays live while it is on the queue.
            self.front = unsafe { at.as_ref() }.next.get();
        }
        // SAFETY: a node stays live while it is on the queue, and the
        // iterator borrows the queue.
        Some(unsafe { A::node(at) })
    }
}

impl<A: Adapter> DoubleEndedIterator for IterPtr<'_, A> {
    #[inline]
    fn next_back(&mut self) -> Option<Self::Item> {
        let at = self.back?;
        if !self.finish() {
            // SAFETY: `at` is not the first node left, so it is not the
            // queue's first either, and its `prev` is the previous node's
            // link.
            self.back = unsafe { at.as_ref().prev.get() };
        }
        // SAFETY: a node stays live while it is on the queue, and the
        // iterator borrows the queue.
        Some(unsafe { A::node(at) })
    }
}

/// An iterator over the nodes of a [`TailQueue`], front to back, or
/// back to front reversed.
pub struct Iter<'head, A: Adapter> {
    nodes: IterPtr<'head, A>,
}

impl<'head, A: Adapter> Iter<'head, A> {
    pub(super) const fn new(nodes: IterPtr<'head, A>) -> Self {
        Self { nodes }
    }
}

impl<'head, A: Adapter> Iterator for Iter<'head, A> {
    type Item = &'head A::Node;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        // SAFETY: a node stays live while it is on the queue, and the
        // iterator borrows the queue.
        self.nodes.next().map(|node| unsafe { node.as_ref() })
    }
}

impl<A: Adapter> DoubleEndedIterator for Iter<'_, A> {
    #[inline]
    fn next_back(&mut self) -> Option<Self::Item> {
        // SAFETY: a node stays live while it is on the queue, and the
        // iterator borrows the queue.
        self.nodes.next_back().map(|node| unsafe { node.as_ref() })
    }
}

/// An iterator over the payloads of the nodes of a [`TailQueue`], front
/// to back, or back to front reversed.
pub struct IterMut<'head, A: Payload> {
    nodes: IterPtr<'head, A>,
    payloads: PhantomData<&'head mut A::Payload>,
}

impl<A: Payload> IterMut<'_, A> {
    pub(super) const fn new(nodes: IterPtr<'_, A>) -> IterMut<'_, A> {
        IterMut {
            nodes,
            payloads: PhantomData,
        }
    }
}

impl<'head, A: Payload> Iterator for IterMut<'head, A> {
    type Item = &'head mut A::Payload;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        // SAFETY: a node stays live while it is on the queue, its payload
        // lies apart from its link, the iterator borrows the queue
        // mutably, and it yields each node once.
        self.nodes
            .next()
            .map(|node| unsafe { A::payload(node).as_mut() })
    }
}

impl<A: Payload> DoubleEndedIterator for IterMut<'_, A> {
    #[inline]
    fn next_back(&mut self) -> Option<Self::Item> {
        // SAFETY: as for `next`.
        self.nodes
            .next_back()
            .map(|node| unsafe { A::payload(node).as_mut() })
    }
}

impl<A: Adapter, C: Count> fmt::Debug for Cursor<'_, '_, A, C> {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Cursor")
            .field("current", &self.current)
            .finish_non_exhaustive()
    }
}

impl<A: Adapter, C: Count> fmt::Debug for CursorMut<'_, '_, A, C> {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CursorMut")
            .field("current", &self.current)
            .finish_non_exhaustive()
    }
}

impl<A: Adapter> fmt::Debug for IterPtr<'_, A> {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IterPtr")
            .field("front", &self.front)
            .field("back", &self.back)
            .finish()
    }
}

impl<A: Payload> fmt::Debug for IterMut<'_, A> {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IterMut")
            .field("nodes", &self.nodes)
            .finish()
    }
}

impl<A: Adapter> fmt::Debug for Iter<'_, A> {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Iter").field("nodes", &self.nodes).finish()
    }
}
