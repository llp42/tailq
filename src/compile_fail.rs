// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Leonardo Lopes Pereira <leonardolopespereira@outlook.com>

//! Misuse the borrow checker and the type system must reject, each block
//! a `compile_fail` doctest with its error code.  Stable rustdoc only
//! checks that a block fails; nightly also checks the code, so CI runs
//! these on nightly.
//!
//! A node that does not live as long as the queue:
//!
//! ```compile_fail,E0597
//! use tailq::{Link, TailQueue};
//! struct Node { link: Link }
//! tailq::adapter!(NodeLink = Node { link });
//! let mut queue = TailQueue::<NodeLink>::new();
//! {
//!     let mut node = Node { link: Link::new() };
//!     queue.push_back(&mut node);
//! }
//! assert!(!queue.is_empty());
//! ```
//!
//! A node declared after its queue, which the queue's drop would touch
//! after the node is gone:
//!
//! ```compile_fail,E0597
//! use tailq::{Link, TailQueue};
//! struct Node { link: Link }
//! tailq::adapter!(NodeLink = Node { link });
//! let mut queue = TailQueue::<NodeLink>::new();
//! let mut node = Node { link: Link::new() };
//! queue.push_back(&mut node);
//! ```
//!
//! Writing, reading or moving a node while it is linked:
//!
//! ```compile_fail,E0506
//! use tailq::{Link, TailQueue};
//! struct Node { key: u32, link: Link }
//! tailq::adapter!(NodeLink = Node { link });
//! let mut node = Node { key: 1, link: Link::new() };
//! let mut queue = TailQueue::<NodeLink>::new();
//! queue.push_back(&mut node);
//! node.key = 2;
//! assert!(!queue.is_empty());
//! ```
//!
//! ```compile_fail,E0503
//! use tailq::{Link, TailQueue};
//! struct Node { key: u32, link: Link }
//! tailq::adapter!(NodeLink = Node { link });
//! let mut node = Node { key: 1, link: Link::new() };
//! let mut queue = TailQueue::<NodeLink>::new();
//! queue.push_back(&mut node);
//! let key = node.key;
//! assert!(!queue.is_empty() && key == 1);
//! ```
//!
//! ```compile_fail,E0505
//! use tailq::{Link, TailQueue};
//! struct Node { link: Link }
//! tailq::adapter!(NodeLink = Node { link });
//! let mut node = Node { link: Link::new() };
//! let mut queue = TailQueue::<NodeLink>::new();
//! queue.push_back(&mut node);
//! let moved = node;
//! assert!(!queue.is_empty() && !moved.link.is_linked());
//! ```
//!
//! One node on two queues through the safe API:
//!
//! ```compile_fail,E0499
//! use tailq::{Link, TailQueue};
//! struct Node { link: Link }
//! tailq::adapter!(NodeLink = Node { link });
//! let mut node = Node { link: Link::new() };
//! let mut one = TailQueue::<NodeLink>::new();
//! let mut two = TailQueue::<NodeLink>::new();
//! one.push_back(&mut node);
//! two.push_back(&mut node);
//! ```
//!
//! Appending a queue of shorter-lived nodes to a longer-lived queue:
//!
//! ```compile_fail,E0597
//! use tailq::{Link, TailQueue};
//! struct Node { link: Link }
//! tailq::adapter!(NodeLink = Node { link });
//! let mut long = Node { link: Link::new() };
//! let mut queue = TailQueue::<NodeLink>::new();
//! queue.push_back(&mut long);
//! {
//!     let mut short = Node { link: Link::new() };
//!     let mut other = TailQueue::<NodeLink>::new();
//!     other.push_back(&mut short);
//!     queue.append(&mut other);
//! }
//! assert!(!queue.is_empty());
//! ```
//!
//! Inserting a shorter-lived node through a cursor:
//!
//! ```compile_fail,E0597
//! use tailq::{Link, TailQueue};
//! struct Node { link: Link }
//! tailq::adapter!(NodeLink = Node { link });
//! let mut queue = TailQueue::<NodeLink>::new();
//! {
//!     let mut short = Node { link: Link::new() };
//!     queue.cursor_front_mut().insert_after(&mut short);
//! }
//! assert!(!queue.is_empty());
//! ```
//!
//! Holding a node the queue lent across a change to the queue:
//!
//! ```compile_fail,E0502
//! use tailq::{Link, TailQueue};
//! struct Node { key: u32, link: Link }
//! tailq::adapter!(NodeLink = Node { link });
//! let mut node = Node { key: 1, link: Link::new() };
//! let mut queue = TailQueue::<NodeLink>::new();
//! queue.push_back(&mut node);
//! let front = queue.front();
//! let _ = queue.pop_front();
//! assert!(front.is_some_and(|node| node.key == 1));
//! ```
//!
//! ```compile_fail,E0502
//! use tailq::{Link, TailQueue};
//! struct Node { key: u32, link: Link }
//! tailq::adapter!(NodeLink = Node { link });
//! let mut node = Node { key: 1, link: Link::new() };
//! let mut queue = TailQueue::<NodeLink>::new();
//! queue.push_back(&mut node);
//! let mut cursor = queue.cursor_front_mut();
//! let current = cursor.current();
//! let _ = cursor.remove_current();
//! assert!(current.is_some_and(|node| node.key == 1));
//! ```
//!
//! Using the queue while a mutable cursor is alive:
//!
//! ```compile_fail,E0502
//! use tailq::{Link, TailQueue};
//! struct Node { link: Link }
//! tailq::adapter!(NodeLink = Node { link });
//! let mut queue = TailQueue::<NodeLink>::new();
//! let mut cursor = queue.cursor_front_mut();
//! assert!(queue.is_empty());
//! cursor.move_next();
//! ```
//!
//! An iterator outliving its queue:
//!
//! ```compile_fail,E0597
//! use tailq::{Link, TailQueue};
//! struct Node { link: Link }
//! tailq::adapter!(NodeLink = Node { link });
//! let iter = {
//!     let queue = TailQueue::<NodeLink>::new();
//!     queue.iter()
//! };
//! assert_eq!(iter.count(), 0);
//! ```
//!
//! Two payloads of one node, or a payload and the node, at once:
//!
//! ```compile_fail,E0499
//! use tailq::{Link, TailQueue};
//! struct Node { link: Link, key: u32 }
//! tailq::adapter!(NodeLink = Node { link, mut key: u32 });
//! let mut node = Node { link: Link::new(), key: 1 };
//! let mut queue = TailQueue::<NodeLink>::new();
//! queue.push_back(&mut node);
//! let one = queue.front_mut();
//! let two = queue.back_mut();
//! assert!(one.is_some() && two.is_some());
//! ```
//!
//! ```compile_fail,E0502
//! use tailq::{Link, TailQueue};
//! struct Node { link: Link, key: u32 }
//! tailq::adapter!(NodeLink = Node { link, mut key: u32 });
//! let mut node = Node { link: Link::new(), key: 1 };
//! let mut queue = TailQueue::<NodeLink>::new();
//! queue.push_back(&mut node);
//! let key = queue.front_mut();
//! let front = queue.front();
//! assert!(key.is_some() && front.is_some());
//! ```
//!
//! A payload iterator cannot be cloned into two that lend one payload:
//!
//! ```compile_fail,E0599
//! use tailq::{Link, TailQueue};
//! struct Node { link: Link, key: u32 }
//! tailq::adapter!(NodeLink = Node { link, mut key: u32 });
//! let mut queue = TailQueue::<NodeLink>::new();
//! let iter = queue.iter_mut();
//! let _ = iter.clone();
//! ```
//!
//! A link cannot be copied, which would put a second link at its place on
//! the queue:
//!
//! ```compile_fail,E0599
//! use tailq::Link;
//! let link = Link::new();
//! let _ = link.clone();
//! ```
//!
//! Queues of nodes that are not `Send`, or not `Sync`, stay on their
//! thread:
//!
//! ```compile_fail,E0277
//! use std::rc::Rc;
//! use tailq::{Link, TailQueue};
//! struct Node { shared: Rc<u32>, link: Link }
//! tailq::adapter!(NodeLink = Node { link });
//! let queue = TailQueue::<'static, NodeLink>::new();
//! std::thread::spawn(move || queue.is_empty());
//! ```
//!
//! ```compile_fail,E0277
//! use core::cell::Cell;
//! use tailq::{Link, TailQueue};
//! struct Node { hits: Cell<u32>, link: Link }
//! tailq::adapter!(NodeLink = Node { link });
//! static QUEUE: TailQueue<'static, NodeLink> = TailQueue::new();
//! ```
//!
//! A link field that only derefs to a link, by reference or through a
//! wrapper:
//!
//! ```compile_fail,E0308
//! use tailq::Link;
//! struct Node { link: &'static Link }
//! tailq::adapter!(NodeLink = Node { link });
//! ```
//!
//! ```compile_fail,E0308
//! use core::ops::Deref;
//! use tailq::Link;
//! static REAL: Link = Link::new();
//! struct Wrapper;
//! impl Deref for Wrapper {
//!     type Target = Link;
//!     fn deref(&self) -> &Link { &REAL }
//! }
//! struct Node { link: Wrapper }
//! tailq::adapter!(NodeLink = Node { link });
//! ```
//!
//! A nested path through a box:
//!
//! ```compile_fail,E0609
//! use tailq::Link;
//! struct Sched { link: Link }
//! struct Node { sched: Box<Sched> }
//! tailq::adapter!(NodeLink = Node { sched.link });
//! ```
//!
//! A payload of another type than its field's, or one inside a packed
//! field, which may leave it unaligned while the link stays aligned:
//!
//! ```compile_fail,E0308
//! use tailq::Link;
//! struct Node { link: Link, key: u32 }
//! tailq::adapter!(NodeLink = Node { link, mut key: u64 });
//! ```
//!
//! ```compile_fail,E0080
//! use tailq::Link;
//! #[repr(C, packed)]
//! struct Packed { tag: u8, key: u32 }
//! struct Node { link: Link, packed: Packed }
//! tailq::adapter!(NodeLink = Node { link, mut packed.key: u32 });
//! ```
//!
//! The macro's own rules, called directly, cannot lend the link as a
//! payload: the queue checks the payload against the adapter's real link.
//!
//! ```compile_fail,E0080
//! use tailq::{Link, TailQueue};
//! struct Node { link: Link }
//! tailq::adapter!(NodeLink = Node { link });
//! tailq::__adapter!(@payload NodeLink [] [] [] Node, [link: Link]);
//! let mut node = Node { link: Link::new() };
//! let mut queue = TailQueue::<NodeLink>::new();
//! queue.push_back(&mut node);
//! let _ = queue.front_mut();
//! ```
//!
//! A brace group in the bounds cannot close the adapter's header and take
//! the place of its body:
//!
//! ```compile_fail
//! use tailq::Link;
//! struct Node { link: Link }
//! struct Dummy;
//! trait Sink { type Node; const OFFSET: usize; }
//! tailq::adapter!(NodeLink = Node { link } where Node: Sized {
//!     type Node = Node;
//!     const OFFSET: usize = 999;
//! } impl Sink for Dummy);
//! ```
//!
//! Only `adapter!` implements `Adapter`: the trait is sealed.
//!
//! ```compile_fail,E0277
//! use tailq::{Adapter, Link};
//! struct Node { link: Link }
//! enum ByHand {}
//! unsafe impl Adapter for ByHand {
//!     type Node = Node;
//!     const OFFSET: usize = 0;
//! }
//! ```
//!
//! A node that leaves is handed back, and dropping it unused is a warning:
//!
//! ```compile_fail
//! #![deny(unused_must_use)]
//! use tailq::{Link, TailQueue};
//! struct Node { link: Link }
//! tailq::adapter!(NodeLink = Node { link });
//! let mut node = Node { link: Link::new() };
//! let mut queue = TailQueue::<NodeLink>::new();
//! queue.push_back(&mut node);
//! queue.cursor_front_mut().remove_current();
//! ```
//!
//! A payload reached through a union, whose other fields could leave it
//! invalid, a `bool` holding 3 for one:
//!
//! ```compile_fail,E0133
//! use tailq::Link;
//! #[derive(Clone, Copy)]
//! union Bits { flag: bool, byte: u8 }
//! struct Node { link: Link, bits: Bits }
//! tailq::adapter!(NodeLink = Node { link, mut bits.flag: bool });
//! ```
//!
//! A packed node is rejected even in a crate whose own `core` has an
//! `assert!` that checks nothing: the check runs in this crate.
//!
//! ```compile_fail,E0080
//! extern crate self as core;
//! pub use std::{clone, fmt, marker};
//! pub mod mem { pub use std::mem::*; }
//! #[macro_export]
//! macro_rules! assert { ($($tokens:tt)*) => {}; }
//! use tailq::Link;
//! #[repr(C, packed)]
//! struct Node { tag: u8, link: Link }
//! tailq::adapter!(NodeLink = Node { link });
//! fn main() {}
//! ```
//!
//! The macro's rules check what they paste into an impl header, even
//! called directly: a brace group in the payload rule's bounds cannot
//! close the header and supply the body,
//!
//! ```compile_fail
//! use tailq::Link;
//! struct Node { link: Link, key: u64 }
//! tailq::adapter!(NodeLink = Node { link });
//! trait Sink { type Payload; const PAYLOAD_OFFSET: usize; }
//! tailq::__adapter!(@payload NodeLink [] [] [
//!     { type Payload = u64; const PAYLOAD_OFFSET: usize = 4096; }
//!     impl Sink for NodeLink
//! ] Node, [key: u64]);
//! ```
//!
//! nor can a `>` in its arguments,
//!
//! ```compile_fail
//! use tailq::Link;
//! struct Node { link: Link, key: u64 }
//! tailq::adapter!(NodeLink = Node { link });
//! trait Sink { type Payload; const PAYLOAD_OFFSET: usize; }
//! tailq::__adapter!(@payload NodeLink [] [
//!     > where { type Payload = &'static u64; const PAYLOAD_OFFSET: usize = 16; }
//!     impl Sink for NodeLink<
//! ] [] Node, [key: u64]);
//! ```
//!
//! nor one in the arguments of the rule that declares the adapter:
//!
//! ```compile_fail
//! use tailq::Link;
//! struct Node { link: Link }
//! struct Other;
//! trait Sink { type Node; const OFFSET: usize; }
//! tailq::__adapter!(@define [NodeLink] [] [
//!     > { type Node = Node; const OFFSET: usize = 4096; } impl Sink for Other<
//! ] = Node { link });
//! ```
//!
//! Review 3, mutant M22: a generic adapter cannot be checked where it is
//! declared, so the queue checks it where it first lends a payload:
//!
//! ```compile_fail,E0080
//! use tailq::{Link, TailQueue};
//! struct Wrapped<T> { link: Link, extra: T }
//! tailq::adapter!(OverLink<T> = Wrapped<T> { link, mut link: Link });
//! let mut queue = TailQueue::<OverLink<u8>>::new();
//! let _ = queue.front_mut();
//! ```
//!
//! Review 3, mutant M24: called directly, the macro's rules cannot measure
//! a payload in another type than the adapter's node, here 64 bytes past
//! the end of a 24-byte node, which the overlap check would pass:
//!
//! ```compile_fail,E0308
//! use tailq::Link;
//! #[repr(C)]
//! struct Node { link: Link, key: u32 }
//! #[repr(C)]
//! struct Fake { link: Link, big: [u8; 64] }
//! tailq::adapter!(NodeLink = Node { link });
//! tailq::__adapter!(@payload NodeLink [] [] [] Fake, [big: [u8; 64]]);
//! ```
//!
//! Mutant M32: an adapter with no parameters is checked where it is
//! declared, even if no queue ever lends its payload:
//!
//! ```compile_fail,E0080
//! use tailq::Link;
//! struct Sched { link: Link, runtime: u64 }
//! struct Thread { sched: Sched }
//! tailq::adapter!(RunLink = Thread { sched.link, mut sched: Sched });
//! ```
//!
//! Requirement audit, e54b390: every operation that hands a node back is
//! `must_use`, not only `remove_current` (mutant M34 drops one):
//!
//! ```compile_fail
//! #![deny(unused_must_use)]
//! use tailq::{Link, TailQueue};
//! struct Node { link: Link }
//! tailq::adapter!(NodeLink = Node { link });
//! let mut queue = TailQueue::<NodeLink>::new();
//! queue.cursor_front_mut().remove_current_ptr();
//! ```
//!
//! ```compile_fail
//! #![deny(unused_must_use)]
//! use tailq::{Link, TailQueue};
//! struct Node { link: Link }
//! tailq::adapter!(NodeLink = Node { link });
//! let mut node = Node { link: Link::new() };
//! let mut queue = TailQueue::<NodeLink>::new();
//! // SAFETY: the node is live, on no queue, and outlives the queue.
//! unsafe { queue.cursor_front_mut().replace_current_ptr((&mut node).into()) };
//! ```
//!
//! ```compile_fail
//! #![deny(unused_must_use)]
//! use tailq::{Link, TailQueue};
//! struct Node { link: Link }
//! tailq::adapter!(NodeLink = Node { link });
//! let mut queue = TailQueue::<NodeLink>::new();
//! queue.pop_back();
//! ```
//!
//! ```compile_fail
//! #![deny(unused_must_use)]
//! use tailq::{Link, TailQueue};
//! struct Node { link: Link }
//! tailq::adapter!(NodeLink = Node { link });
//! let mut queue = TailQueue::<NodeLink>::new();
//! queue.pop_front_ptr();
//! ```
//!
//! A method of the caller's cannot stand in for the slot's pointer, which a
//! method call would reach first: on nightly, a const trait method could
//! return a pointer to another type, and the checks would look at that
//! type while the offset measured the node.
//!
//! ```compile_fail,E0308
//! #![feature(const_trait_impl)]
//! use core::mem::MaybeUninit;
//! use tailq::Link;
//! #[repr(C)]
//! struct Fake { link: Link }
//! #[repr(C)]
//! struct Node { link: &'static u64, rest: Option<&'static u64> }
//! const trait Hijack { fn as_mut_ptr(self) -> *mut Fake; }
//! const impl Hijack for MaybeUninit<Node> {
//!     fn as_mut_ptr(self) -> *mut Fake { core::ptr::dangling_mut() }
//! }
//! tailq::adapter!(NodeLink = Node { link });
//! ```
//!
//! nor make the payload rule, called directly, measure a payload in
//! another type than the adapter's node:
//!
//! ```compile_fail,E0308
//! #![feature(const_trait_impl)]
//! use core::mem::MaybeUninit;
//! use tailq::Link;
//! #[repr(C)]
//! struct Node { big: [u8; 8], link: Link }
//! #[repr(C)]
//! struct Fake { pad: [u8; 4096], big: [u8; 8] }
//! const trait Hijack { fn as_mut_ptr(self) -> *mut Node; }
//! const impl Hijack for MaybeUninit<Fake> {
//!     fn as_mut_ptr(self) -> *mut Node { core::ptr::dangling_mut() }
//! }
//! tailq::adapter!(NodeLink = Node { link });
//! tailq::__adapter!(@payload NodeLink [] [] [] Fake, [big: [u8; 8]]);
//! ```
//!
//! Review 4, mutant T01: a packed node fails even with its link first, at
//! offset 0, where only the node's own alignment shows that the link is
//! unaligned, as it is in every node of an array but the first:
//!
//! ```compile_fail,E0080
//! use tailq::Link;
//! #[repr(C, packed)]
//! struct Node { link: Link, tag: u8 }
//! tailq::adapter!(NodeLink = Node { link });
//! ```
//!
//! Review 5, mutant A05: a payload field that only derefs to the payload's
//! type fails to compile, here a `Box<u64>` that would be lent as the
//! `u64` of its pointer, which safe code could then overwrite.  The raw
//! place proves the field's type; the reference check alone, a closure's
//! return and so a coercion site, would let the box deref to its `u64`:
//!
//! ```compile_fail,E0308
//! use tailq::Link;
//! struct Node { link: Link, key: Box<u64> }
//! tailq::adapter!(NodeLink = Node { link, mut key: u64 });
//! ```
//!
//! On nightly, `offset_of!` walks into an enum's variant, which a field
//! access cannot, so the raw place went through a const `DerefMut` to
//! another type's link while the offset was that of a reference; the
//! place must now lie where `offset_of!` says:
//!
//! ```compile_fail,E0080
//! #![feature(offset_of_enum, const_trait_impl, const_convert)]
//! #![allow(non_snake_case, dangerous_implicit_autorefs)]
//! use core::ops::{Deref, DerefMut};
//! use tailq::Link;
//! struct Inner { link: Link }
//! struct Shadow { V: Inner }
//! enum Node { V { link: (&'static u64, u64), shadow: Shadow } }
//! const impl Deref for Node {
//!     type Target = Shadow;
//!     fn deref(&self) -> &Shadow { let Node::V { shadow, .. } = self; shadow }
//! }
//! const impl DerefMut for Node {
//!     fn deref_mut(&mut self) -> &mut Shadow { let Node::V { shadow, .. } = self; shadow }
//! }
//! tailq::adapter!(NodeLink = Node { V.link });
//! ```
//!
//! nor with a `DerefMut` that first writes the slot's variant, so that a
//! link lies at the very offset of the variant field `offset_of!` names:
//!
//! ```compile_fail,E0080
//! #![feature(offset_of_enum, const_trait_impl, const_convert)]
//! #![allow(non_snake_case, dangerous_implicit_autorefs)]
//! use core::ops::{Deref, DerefMut};
//! use tailq::Link;
//! struct Inner { link: Link }
//! struct Shadow { V: Inner }
//! enum Node { V { link: (&'static u64, u64) }, W { shadow: Shadow } }
//! const impl Deref for Node {
//!     type Target = Shadow;
//!     fn deref(&self) -> &Shadow {
//!         match self { Node::W { shadow } => shadow, Node::V { .. } => panic!() }
//!     }
//! }
//! const impl DerefMut for Node {
//!     fn deref_mut(&mut self) -> &mut Shadow {
//!         *self = Node::W { shadow: Shadow { V: Inner { link: Link::new() } } };
//!         match self { Node::W { shadow } => shadow, Node::V { .. } => panic!() }
//!     }
//! }
//! tailq::adapter!(NodeLink = Node { V.link });
//! ```
//!
//! Review 6: the link's places sit in an `unsafe` block, so a path could
//! take a union field there and deref its `ManuallyDrop`, which a reference
//! to the field outside the block now rejects.  Here a one-variant enum
//! derefs, reading nothing, to its own field, a union; the place reaches a
//! link through the union's `V` while `offset_of!` measures its `link: ()`,
//! and a node built as `Pun { link: () }` holds no link at `OFFSET`:
//!
//! ```compile_fail,E0133
//! #![feature(offset_of_enum, const_trait_impl, const_convert)]
//! #![allow(non_snake_case, dangerous_implicit_autorefs)]
//! use core::mem::ManuallyDrop;
//! use core::ops::{Deref, DerefMut};
//! use tailq::Link;
//! struct Inner { link: Link }
//! struct Shadow { x: Inner }
//! union Pun { V: ManuallyDrop<Shadow>, link: () }
//! enum Node { V { x: Pun } }
//! const impl Deref for Node {
//!     type Target = Pun;
//!     fn deref(&self) -> &Pun { let Node::V { x } = self; x }
//! }
//! const impl DerefMut for Node {
//!     fn deref_mut(&mut self) -> &mut Pun { let Node::V { x } = self; x }
//! }
//! tailq::adapter!(NodeLink = Node { V.x.link });
//! ```
//!
//! Review 6, mutants A04 to A06: the payload's place, too, is taken
//! through shared derefs only, on a slot nothing writes, and must lie where
//! `offset_of!` says.  On nightly a payload path through an enum's variant
//! reached through a const `DerefMut` would otherwise lend a `&mut u64`
//! over a `&'static u64`:
//!
//! ```compile_fail,E0080
//! #![feature(offset_of_enum, const_trait_impl, const_convert)]
//! #![allow(non_snake_case, dangerous_implicit_autorefs)]
//! use core::ops::{Deref, DerefMut};
//! use tailq::Link;
//! struct Inner { key: u64 }
//! struct Shadow { V: Inner }
//! enum Payload { V { key: &'static u64, shadow: Shadow } }
//! struct Node { link: Link, payload: Payload }
//! const impl Deref for Payload {
//!     type Target = Shadow;
//!     fn deref(&self) -> &Shadow { let Payload::V { shadow, .. } = self; shadow }
//! }
//! const impl DerefMut for Payload {
//!     fn deref_mut(&mut self) -> &mut Shadow { let Payload::V { shadow, .. } = self; shadow }
//! }
//! tailq::adapter!(NodeLink = Node { link, mut payload.V.key: u64 });
//! ```
//!
//! nor with a `DerefMut` that first writes the slot's variant, so that the
//! payload it reaches lies at the very offset of the variant field
//! `offset_of!` names:
//!
//! ```compile_fail,E0080
//! #![feature(offset_of_enum, const_trait_impl, const_convert)]
//! #![allow(non_snake_case, dangerous_implicit_autorefs)]
//! use core::ops::{Deref, DerefMut};
//! use tailq::Link;
//! struct Inner { key: u64 }
//! struct Shadow { V: Inner }
//! enum Payload { V { key: &'static u64 }, W { shadow: Shadow } }
//! struct Node { link: Link, payload: Payload }
//! const impl Deref for Payload {
//!     type Target = Shadow;
//!     fn deref(&self) -> &Shadow {
//!         match self { Payload::W { shadow } => shadow, Payload::V { .. } => panic!() }
//!     }
//! }
//! const impl DerefMut for Payload {
//!     fn deref_mut(&mut self) -> &mut Shadow {
//!         *self = Payload::W { shadow: Shadow { V: Inner { key: 0 } } };
//!         match self { Payload::W { shadow } => shadow, Payload::V { .. } => panic!() }
//!     }
//! }
//! tailq::adapter!(NodeLink = Node { link, mut payload.V.key: u64 });
//! ```
//!
//! Review 7: called directly, the payload rule checks that its node is the
//! adapter's without a coercion, which would let the node's pointer unsize
//! to one to a trait object:
//!
//! ```compile_fail,E0308
//! use tailq::Link;
//! struct Node { link: Link, key: u64 }
//! tailq::adapter!(NodeLink = Node { link });
//! tailq::__adapter!(@payload NodeLink [] [] [] dyn core::any::Any, [key: u64]);
//! ```
//!
//! Review 7 found these rejected, and pins them.  On nightly, a link or a
//! payload in a field that is unsafe to reach, which only the reference
//! taken outside the macro's `unsafe` block rejects:
//!
//! ```compile_fail,E0133
//! #![feature(unsafe_fields)]
//! #![allow(incomplete_features)]
//! use tailq::Link;
//! struct Node { tag: u64, unsafe link: Link }
//! tailq::adapter!(NodeLink = Node { link });
//! ```
//!
//! ```compile_fail,E0133
//! #![feature(unsafe_fields)]
//! #![allow(incomplete_features)]
//! use tailq::Link;
//! struct Node { link: Link, unsafe key: u64 }
//! tailq::adapter!(NodeLink = Node { link, mut key: u64 });
//! ```
//!
//! A generic packed node, whose layout the alignment check cannot see
//! until a queue names its parameter, which the reference to its link
//! rejects where it is declared:
//!
//! ```compile_fail,E0793
//! use tailq::Link;
//! #[repr(C, packed)]
//! struct Node<T: Copy> { t: T, link: Link }
//! tailq::adapter!(NodeLink<T> = Node<T> { link } where T: Copy);
//! ```
