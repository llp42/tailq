// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Leonardo Lopes Pereira <leonardolopespereira@outlook.com>

//! Cursors and the double-ended iterators over a [`TailQueue`].
//!
//! A cursor rests on a node or on the ghost, the position before the
//! front and after the back: moving forward from the last node, or back
//! from the first, reaches the ghost, and moving on from the ghost wraps
//! to the other end.

use super::adapter::{link_of, node_of, payload_of};
use super::{Adapter, Count, Counted, Link, Payload, TailQueue, Uncounted, is};
use core::fmt;
use core::iter::FusedIterator;
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
        self.current.map(|link| unsafe { node_of::<A>(link) })
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
            .map(|link| unsafe { node_of::<A>(link) })
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
            .map(|link| unsafe { node_of::<A>(link) })
    }
}

// By hand: a derive would require the adapter and the count to be Clone.
impl<A: Adapter, C: Count> Clone for Cursor<'_, '_, A, C> {
    #[inline]
    fn clone(&self) -> Self {
        *self
    }
}

impl<A: Adapter, C: Count> Copy for Cursor<'_, '_, A, C> {}

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
            Some(at) => unsafe {
                self.queue.link_after(at, link_of::<A>(node));
            },
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
            Some(at) => unsafe {
                self.queue.link_before(at, link_of::<A>(node));
            },
        }
    }

    /// Unlinks the node at the cursor and returns it, moving to the next
    /// node; `None` at the ghost.
    ///
    /// # Panics
    ///
    /// In debug builds, when the cursor's link is not consistent with its
    /// neighbours: the queue is corrupt.
    #[must_use]
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
    #[must_use]
    #[inline]
    pub fn remove_current_ptr(&mut self) -> Option<NonNull<A::Node>> {
        let at = self.current?;
        // SAFETY: the cursor rests on a node of the queue it borrows
        // exclusively, and that node is live.
        unsafe {
            let next = at.as_ref().next.get();
            self.queue.unlink(at);
            self.current = next;
            Some(node_of::<A>(at))
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
            let link = link_of::<A>(NonNull::from(node));
            self.queue.replace(at, link);
            self.current = Some(link);
            Ok(node_of::<A>(at).as_mut())
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
    #[must_use]
    #[inline]
    pub unsafe fn replace_current_ptr(
        &mut self,
        node: NonNull<A::Node>,
    ) -> Option<NonNull<A::Node>> {
        let at = self.current?;
        // SAFETY: the caller keeps the push contract, and the cursor
        // rests on a node of the queue it borrows exclusively.
        unsafe {
            let link = link_of::<A>(node);
            self.queue.replace(at, link);
            self.current = Some(link);
            Some(node_of::<A>(at))
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
            .map(|link| unsafe { payload_of::<A>(node_of::<A>(link)).as_mut() })
    }
}

/// An iterator over the pointers the nodes of a [`TailQueue`] were
/// pushed with, front to back, or back to front reversed.
pub struct IterPtr<'head, A: Adapter, C: Count = Uncounted> {
    front: Option<NonNull<Link>>,
    back: Option<NonNull<Link>>,
    /// The nodes left, for a counted queue; nothing otherwise.
    left: C,
    nodes: PhantomData<&'head A::Node>,
}

impl<A: Adapter, C: Count> IterPtr<'_, A, C> {
    pub(super) const fn new(
        front: Option<NonNull<Link>>,
        back: Option<NonNull<Link>>,
        left: C,
    ) -> Self {
        Self {
            front,
            back,
            left,
            nodes: PhantomData,
        }
    }

    /// Ends the walk if the two ends have met on the last node left,
    /// and returns whether they had.
    #[inline]
    fn finish(
        &mut self,
        at: NonNull<Link>,
        other: Option<NonNull<Link>>,
    ) -> bool {
        let met = is(other, at);
        if met {
            self.front = None;
            self.back = None;
        }
        met
    }
}

impl<A: Adapter, C: Count> Iterator for IterPtr<'_, A, C> {
    type Item = NonNull<A::Node>;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        let at = self.front?;
        self.left.sub_one();
        if !self.finish(at, self.back) {
            // SAFETY: a node stays live while it is on the queue, and `at`
            // is not the last node left, so it is not the queue's last
            // either and has a next node.
            self.front =
                Some(unsafe { at.as_ref().next.get().unwrap_unchecked() });
        }
        // SAFETY: a node stays live while it is on the queue, and the
        // iterator borrows the queue.
        Some(unsafe { node_of::<A>(at) })
    }

    #[inline]
    fn fold<B, F>(self, init: B, mut f: F) -> B
    where
        F: FnMut(B, Self::Item) -> B,
    {
        let (Some(mut at), Some(back)) = (self.front, self.back) else {
            return init;
        };
        let mut acc = init;
        loop {
            // SAFETY: a node stays live while it is on the queue, and the
            // iterator borrows the queue.
            acc = f(acc, unsafe { node_of::<A>(at) });
            if at.as_ptr() == back.as_ptr() {
                return acc;
            }
            // SAFETY: `at` is not the last node left, so it is not the
            // queue's last either and has a next node.
            at = unsafe { at.as_ref().next.get().unwrap_unchecked() };
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let Some(front) = self.front else {
            return (0, Some(0));
        };
        match self.left.exact() {
            Some(left) => (left, Some(left)),
            None if is(self.back, front) => (1, Some(1)),
            None => (2, None),
        }
    }

    #[inline]
    fn count(self) -> usize {
        match (self.front, self.left.exact()) {
            (None, _) => 0,
            (Some(_), Some(left)) => left,
            (Some(_), None) => self.fold(0, |n, _| n.wrapping_add(1)),
        }
    }

    #[inline]
    fn last(self) -> Option<Self::Item> {
        // SAFETY: a node stays live while it is on the queue, and the
        // iterator borrows the queue.
        self.back.map(|at| unsafe { node_of::<A>(at) })
    }
}

impl<A: Adapter, C: Count> DoubleEndedIterator for IterPtr<'_, A, C> {
    #[inline]
    fn rfold<B, F>(self, init: B, mut f: F) -> B
    where
        F: FnMut(B, Self::Item) -> B,
    {
        let (Some(front), Some(mut at)) = (self.front, self.back) else {
            return init;
        };
        let mut acc = init;
        loop {
            // SAFETY: as in `fold`.
            acc = f(acc, unsafe { node_of::<A>(at) });
            if at.as_ptr() == front.as_ptr() {
                return acc;
            }
            // SAFETY: `at` is not the first node left, so it is not the
            // queue's first either and its `prev` is the previous link.
            at = unsafe { at.as_ref().prev.get().unwrap_unchecked() };
        }
    }

    #[inline]
    fn next_back(&mut self) -> Option<Self::Item> {
        let at = self.back?;
        self.left.sub_one();
        if !self.finish(at, self.front) {
            // SAFETY: `at` is not the first node left, so it is not the
            // queue's first either, and its `prev` is the previous node's
            // link.
            self.back =
                Some(unsafe { at.as_ref().prev.get().unwrap_unchecked() });
        }
        // SAFETY: a node stays live while it is on the queue, and the
        // iterator borrows the queue.
        Some(unsafe { node_of::<A>(at) })
    }
}

impl<A: Adapter, C: Count> FusedIterator for IterPtr<'_, A, C> {}

impl<A: Adapter> ExactSizeIterator for IterPtr<'_, A, Counted> {}

impl<A: Adapter, C: Count> Clone for IterPtr<'_, A, C> {
    #[inline]
    fn clone(&self) -> Self {
        Self::new(self.front, self.back, self.left)
    }
}

/// An iterator over the nodes of a [`TailQueue`], front to back, or
/// back to front reversed.
pub struct Iter<'head, A: Adapter, C: Count = Uncounted> {
    nodes: IterPtr<'head, A, C>,
}

impl<'head, A: Adapter, C: Count> Iter<'head, A, C> {
    pub(super) const fn new(nodes: IterPtr<'head, A, C>) -> Self {
        Self { nodes }
    }
}

impl<'head, A: Adapter, C: Count> Iterator for Iter<'head, A, C> {
    type Item = &'head A::Node;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        // SAFETY: a node stays live while it is on the queue, and the
        // iterator borrows the queue.
        self.nodes.next().map(|node| unsafe { node.as_ref() })
    }

    #[inline]
    fn fold<B, F>(self, init: B, mut f: F) -> B
    where
        F: FnMut(B, Self::Item) -> B,
    {
        // SAFETY: as for `next`.
        self.nodes
            .fold(init, |acc, node| f(acc, unsafe { node.as_ref() }))
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.nodes.size_hint()
    }

    #[inline]
    fn count(self) -> usize {
        self.nodes.count()
    }

    #[inline]
    fn last(self) -> Option<Self::Item> {
        // SAFETY: a node stays live while it is on the queue, and the
        // iterator borrows the queue.
        self.nodes.last().map(|node| unsafe { node.as_ref() })
    }
}

impl<A: Adapter, C: Count> DoubleEndedIterator for Iter<'_, A, C> {
    #[inline]
    fn rfold<B, F>(self, init: B, mut f: F) -> B
    where
        F: FnMut(B, Self::Item) -> B,
    {
        // SAFETY: as for `next`.
        self.nodes
            .rfold(init, |acc, node| f(acc, unsafe { node.as_ref() }))
    }

    #[inline]
    fn next_back(&mut self) -> Option<Self::Item> {
        // SAFETY: a node stays live while it is on the queue, and the
        // iterator borrows the queue.
        self.nodes.next_back().map(|node| unsafe { node.as_ref() })
    }
}

impl<A: Adapter, C: Count> FusedIterator for Iter<'_, A, C> {}

impl<A: Adapter> ExactSizeIterator for Iter<'_, A, Counted> {}

impl<A: Adapter, C: Count> Clone for Iter<'_, A, C> {
    #[inline]
    fn clone(&self) -> Self {
        Self::new(self.nodes.clone())
    }
}

/// An iterator over the payloads of the nodes of a [`TailQueue`], front
/// to back, or back to front reversed.
pub struct IterMut<'head, A: Payload, C: Count = Uncounted> {
    nodes: IterPtr<'head, A, C>,
    payloads: PhantomData<&'head mut A::Payload>,
}

impl<A: Payload, C: Count> IterMut<'_, A, C> {
    pub(super) const fn new(nodes: IterPtr<'_, A, C>) -> IterMut<'_, A, C> {
        IterMut {
            nodes,
            payloads: PhantomData,
        }
    }
}

impl<'head, A: Payload, C: Count> Iterator for IterMut<'head, A, C> {
    type Item = &'head mut A::Payload;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        // SAFETY: a node stays live while it is on the queue, its payload
        // lies apart from its link, the iterator borrows the queue
        // mutably, and it yields each node once.
        self.nodes
            .next()
            .map(|node| unsafe { payload_of::<A>(node).as_mut() })
    }

    #[inline]
    fn fold<B, F>(self, init: B, mut f: F) -> B
    where
        F: FnMut(B, Self::Item) -> B,
    {
        self.nodes.fold(init, |acc, node| {
            // SAFETY: as for `next`.
            f(acc, unsafe { payload_of::<A>(node).as_mut() })
        })
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.nodes.size_hint()
    }

    #[inline]
    fn count(self) -> usize {
        self.nodes.count()
    }

    #[inline]
    fn last(self) -> Option<Self::Item> {
        // SAFETY: as for `next`.
        self.nodes
            .last()
            .map(|node| unsafe { payload_of::<A>(node).as_mut() })
    }
}

impl<A: Payload, C: Count> DoubleEndedIterator for IterMut<'_, A, C> {
    #[inline]
    fn rfold<B, F>(self, init: B, mut f: F) -> B
    where
        F: FnMut(B, Self::Item) -> B,
    {
        self.nodes.rfold(init, |acc, node| {
            // SAFETY: as for `next`.
            f(acc, unsafe { payload_of::<A>(node).as_mut() })
        })
    }

    #[inline]
    fn next_back(&mut self) -> Option<Self::Item> {
        // SAFETY: as for `next`.
        self.nodes
            .next_back()
            .map(|node| unsafe { payload_of::<A>(node).as_mut() })
    }
}

impl<A: Payload, C: Count> FusedIterator for IterMut<'_, A, C> {}

impl<A: Payload> ExactSizeIterator for IterMut<'_, A, Counted> {}

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

impl<A: Adapter, C: Count> fmt::Debug for IterPtr<'_, A, C> {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IterPtr")
            .field("front", &self.front)
            .field("back", &self.back)
            .finish()
    }
}

impl<A: Payload, C: Count> fmt::Debug for IterMut<'_, A, C> {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IterMut")
            .field("nodes", &self.nodes)
            .finish()
    }
}

impl<A: Adapter, C: Count> fmt::Debug for Iter<'_, A, C> {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Iter").field("nodes", &self.nodes).finish()
    }
}
