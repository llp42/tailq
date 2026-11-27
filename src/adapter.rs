// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Leonardo Lopes Pereira <leonardolopespereira@outlook.com>

//! How a queue finds a node's link: the adapter trait and the
//! [`adapter!`](crate::adapter!) macro that implements it.

use super::Link;
use core::ptr::NonNull;

/// Maps a node type to the [`Link`] it embeds, and back.
///
/// [`adapter!`](crate::adapter!) declares an adapter type and implements
/// it; implement it by hand only for what the macro cannot express.  An
/// adapter is a type a queue names as its type parameter and never holds
/// a value of, so it is best uninhabited.
///
/// ```
/// use core::convert::Infallible;
/// use core::mem::offset_of;
/// use core::ptr::NonNull;
/// use tailq::{Adapter, Link, TailQueue};
///
/// struct Thread {
///     id: u32,
///     link: Link,
/// }
///
/// struct ThreadLink(Infallible);
///
/// // SAFETY: both directions step by the offset of `link`, an aligned
/// // `Link` field, from the pointer they are given.
/// unsafe impl Adapter for ThreadLink {
///     type Node = Thread;
///
///     unsafe fn link(node: NonNull<Thread>) -> NonNull<Link> {
///         unsafe { NonNull::new_unchecked(&raw mut (*node.as_ptr()).link) }
///     }
///
///     unsafe fn node(link: NonNull<Link>) -> NonNull<Thread> {
///         unsafe { link.byte_sub(offset_of!(Thread, link)).cast() }
///     }
/// }
///
/// type RunQueue<'nodes> = TailQueue<'nodes, ThreadLink>;
/// ```
///
/// # Safety
///
/// `link` must return a pointer to an aligned [`Link`] inside the node it
/// is given, at the same offset for every node, and `node` must invert
/// it; both must derive their result from the pointer they are given, so
/// that it keeps that pointer's access.
pub unsafe trait Adapter {
    /// The node type that embeds the link.
    type Node;

    /// Returns the link embedded in `node`.
    ///
    /// # Safety
    ///
    /// `node` must point at a live node.
    unsafe fn link(node: NonNull<Self::Node>) -> NonNull<Link>;

    /// Returns the node that embeds `link`.
    ///
    /// # Safety
    ///
    /// `link` must be the link of a live node.
    unsafe fn node(link: NonNull<Link>) -> NonNull<Self::Node>;
}

/// Declares an [`Adapter`]: an uninhabited type that links a node type
/// through one of its fields.
///
/// ```
/// use tailq::{Counted, Link, TailQueue};
///
/// pub struct Thread {
///     id: u32,
///     run_queue_node: Link,
/// }
///
/// tailq::adapter!(
///     /// Links a thread into the queue of threads ready to run.
///     pub RunLink = Thread { run_queue_node }
/// );
///
/// /// The threads ready to run.
/// pub type RunQueue<'nodes> = TailQueue<'nodes, RunLink>;
/// /// The same, with an O(1) length.
/// pub type CountedRunQueue<'nodes> = TailQueue<'nodes, RunLink, Counted>;
///
/// let mut thread = Thread { id: 7, run_queue_node: Link::new() };
/// let mut run_queue = CountedRunQueue::new();
/// run_queue.push_back(&mut thread);
/// assert_eq!(run_queue.len(), 1);
/// ```
///
/// One node joins several queues at once through several fields, each
/// with its adapter.  A link inside a nested field takes its path,
/// `Thread { sched.link }`.  A generic node takes the adapter's type
/// parameters after its name, and their bounds in a trailing `where`:
///
/// ```
/// use core::marker::PhantomData;
/// use tailq::{Link, TailQueue};
///
/// trait Platform {}
///
/// struct Sched {
///     link: Link,
/// }
///
/// struct Thread<P: Platform> {
///     platform: PhantomData<P>,
///     sched: Sched,
/// }
///
/// tailq::adapter!(SchedLink<P> = Thread<P> { sched.link } where P: Platform);
///
/// type RunQueue<'nodes, P> = TailQueue<'nodes, SchedLink<P>>;
/// ```
///
/// A queue whose head lives in a `static` takes `'static` nodes, such as
/// leaked ones, or nodes pushed by pointer, and is reached through a lock:
///
/// ```
/// use std::sync::Mutex;
/// use tailq::{Link, TailQueue};
///
/// struct Thread {
///     id: u32,
///     link: Link,
/// }
///
/// tailq::adapter!(RunLink = Thread { link });
///
/// static RUN_QUEUE: Mutex<TailQueue<'static, RunLink>> =
///     Mutex::new(TailQueue::new());
///
/// let thread = Box::leak(Box::new(Thread { id: 1, link: Link::new() }));
/// let mut run_queue = RUN_QUEUE.lock().unwrap();
/// run_queue.push_back(thread);
/// assert_eq!(run_queue.front().map(|thread| thread.id), Some(1));
/// ```
///
/// The field must have type [`Link`] itself; any other type fails to
/// compile, including one that only derefs to a link:
///
/// ```compile_fail,E0308
/// struct Thread {
///     link: u64,
/// }
///
/// tailq::adapter!(RunLink = Thread { link });
/// ```
///
/// ```compile_fail,E0308
/// use tailq::Link;
///
/// struct Thread {
///     link: Box<Link>,
/// }
///
/// tailq::adapter!(RunLink = Thread { link });
/// ```
///
/// A packed node, whose link could be unaligned, fails to compile too:
///
/// ```compile_fail,E0793
/// use tailq::Link;
///
/// #[repr(C, packed)]
/// struct Thread {
///     tag: u8,
///     link: Link,
/// }
///
/// tailq::adapter!(RunLink = Thread { link });
/// ```
#[macro_export]
macro_rules! adapter {
    (
        $(#[$attr:meta])*
        $vis:vis $name:ident $(<$($param:ident),+ $(,)?>)?
            = $node:ty { $($field:ident).+ $(,)? }
            $(where $($bound:tt)+)?
    ) => {
        $(#[$attr])*
        #[derive(Clone, Copy, Debug)]
        $vis struct $name $(<$($param),+>)?(
            ::core::convert::Infallible,
            ::core::marker::PhantomData<fn() -> ($($($param,)+)?)>,
        );

        // SAFETY: `link` projects the field, which the raw place proves is
        // a `Link` and the reference proves is aligned, from the pointer it
        // is given, and `node` steps back by the field's offset.
        unsafe impl $(<$($param),+>)? $crate::Adapter for $name $(<$($param),+>)?
        $(where $($bound)+)?
        {
            type Node = $node;

            #[inline]
            unsafe fn link(
                node: ::core::ptr::NonNull<$node>,
            ) -> ::core::ptr::NonNull<$crate::Link> {
                // A reference to the field: rejects a packed node.
                let _: fn(&$node) -> &$crate::Link = |node| &node.$($field).+;
                // A raw place, which is never deref-coerced: rejects a
                // field that only derefs to a link.
                // SAFETY: the node is live.
                let link: *mut $crate::Link =
                    unsafe { &raw mut (*node.as_ptr()).$($field).+ };
                // SAFETY: a field of a live node is not null.
                unsafe { ::core::ptr::NonNull::new_unchecked(link) }
            }

            #[inline]
            unsafe fn node(
                link: ::core::ptr::NonNull<$crate::Link>,
            ) -> ::core::ptr::NonNull<$node> {
                // SAFETY: the link lies this far into its live node.
                unsafe {
                    link.byte_sub(::core::mem::offset_of!($node, $($field).+))
                        .cast()
                }
            }
        }
    };
}
