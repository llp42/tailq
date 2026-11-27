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

/// An [`Adapter`] whose nodes hold a payload apart from their link, which
/// a queue lends as `&mut` while the node is on it: through
/// [`front_mut`](crate::TailQueue::front_mut),
/// [`back_mut`](crate::TailQueue::back_mut),
/// [`iter_mut`](crate::TailQueue::iter_mut) and
/// [`current_mut`](crate::CursorMut::current_mut).
///
/// [`adapter!`](crate::adapter!) implements it when the field list names
/// the payload after the link, `Thread { link, mut info: Info }`.  A
/// queue hands out only `&A::Node` otherwise, since a `&mut` to the
/// whole node could overwrite its link.
///
/// A payload may hold the links of other queues.  A node is on several
/// queues only through pushes by pointer, and their contract then makes
/// the caller keep those queues off the node while this one lends its
/// payload, and keep the payload's borrower off those links.
///
/// # Safety
///
/// `payload` must return a pointer to an aligned `Self::Payload` inside
/// the node it is given, derived from that pointer, that does not overlap
/// the [`Link`] that [`Adapter::link`] finds.
pub unsafe trait Payload: Adapter {
    /// The part of a node a queue lends mutably.
    type Payload;

    /// Returns the payload of `node`.
    ///
    /// # Safety
    ///
    /// `node` must point at a live node.
    unsafe fn payload(node: NonNull<Self::Node>) -> NonNull<Self::Payload>;
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
/// A payload, which the queue lends as `&mut` while the node is linked,
/// follows the link as `mut path: Type`, and makes the adapter a
/// [`Payload`] one:
///
/// ```
/// use tailq::{Link, TailQueue};
///
/// struct Info {
///     id: u32,
///     runtime: u64,
/// }
///
/// struct Thread {
///     link: Link,
///     info: Info,
/// }
///
/// tailq::adapter!(RunLink = Thread { link, mut info: Info });
///
/// let mut threads = [1, 2].map(|id| Thread {
///     link: Link::new(),
///     info: Info { id, runtime: 0 },
/// });
/// let mut run_queue: TailQueue<'_, RunLink> = threads.iter_mut().collect();
/// for info in run_queue.iter_mut() {
///     info.runtime += u64::from(info.id);
/// }
/// if let Some(info) = run_queue.front_mut() {
///     info.runtime *= 10;
/// }
/// let runtimes: Vec<u64> = run_queue.iter().map(|t| t.info.runtime).collect();
/// assert_eq!(runtimes, [10, 2]);
/// ```
///
/// The payload must not overlap the link, which the first use of the
/// adapter's payload checks at compile time:
///
/// ```compile_fail,E0080
/// use tailq::{Link, TailQueue};
///
/// struct Sched {
///     link: Link,
///     runtime: u64,
/// }
///
/// struct Thread {
///     sched: Sched,
/// }
///
/// tailq::adapter!(RunLink = Thread { sched.link, mut sched: Sched });
///
/// let mut queue = TailQueue::<RunLink>::new();
/// let _ = queue.front_mut();
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
            = $node:ty {
                $($field:ident).+
                $(, mut $($payload:ident).+ : $payload_ty:ty)?
                $(,)?
            }
            $(where $($bound:tt)+)?
    ) => {
        $(#[$attr])*
        #[derive(Clone, Copy, Debug)]
        $vis struct $name $(<$($param),+>)?(
            ::core::convert::Infallible,
            ::core::marker::PhantomData<fn() -> ($($($param,)+)?)>,
        );

        $crate::adapter!(
            @link $name [$($($param),+)?] [$($($bound)+)?] $node,
                [$($field).+]
        );
        $crate::adapter!(
            @payload $name [$($($param),+)?] [$($($bound)+)?] $node,
                [$($field).+], [$($($payload).+ : $payload_ty)?]
        );
    };
    (
        @link $name:ident [$($param:ident),*] [$($bound:tt)*] $node:ty,
            [$($field:ident).+]
    ) => {
        // SAFETY: `link` projects the field, which the raw place proves is
        // a `Link` and the reference proves is aligned, from the pointer it
        // is given, and `node` steps back by the field's offset.
        unsafe impl<$($param),*> $crate::Adapter for $name<$($param),*>
        where
            $($bound)*
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
    (
        @payload $name:ident [$($param:ident),*] [$($bound:tt)*] $node:ty,
            [$($field:ident).+], []
    ) => {};
    (
        @payload $name:ident [$($param:ident),*] [$($bound:tt)*] $node:ty,
            [$($field:ident).+], [$($payload:ident).+ : $payload_ty:ty]
    ) => {
        // SAFETY: `payload` projects the field, which the raw place proves
        // is a `$payload_ty` and the reference proves is aligned, from the
        // pointer it is given, and the assertion proves it lies apart from
        // the link.
        unsafe impl<$($param),*> $crate::Payload for $name<$($param),*>
        where
            $($bound)*
        {
            type Payload = $payload_ty;

            #[inline]
            unsafe fn payload(
                node: ::core::ptr::NonNull<$node>,
            ) -> ::core::ptr::NonNull<$payload_ty> {
                const {
                    let link = ::core::mem::offset_of!($node, $($field).+);
                    let payload =
                        ::core::mem::offset_of!($node, $($payload).+);
                    ::core::assert!(
                        payload + ::core::mem::size_of::<$payload_ty>() <= link
                            || link + ::core::mem::size_of::<$crate::Link>()
                                <= payload,
                        "tailq::adapter!: the payload overlaps the link",
                    );
                }
                // A reference to the field: rejects a packed node.
                let _: fn(&$node) -> &$payload_ty =
                    |node| &node.$($payload).+;
                // A raw place, which is never deref-coerced: proves the
                // field's type.
                // SAFETY: the node is live.
                let payload: *mut $payload_ty =
                    unsafe { &raw mut (*node.as_ptr()).$($payload).+ };
                // SAFETY: a field of a live node is not null.
                unsafe { ::core::ptr::NonNull::new_unchecked(payload) }
            }
        }
    };
}
