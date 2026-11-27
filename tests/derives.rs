// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Leonardo Lopes Pereira <leonardolopespereira@outlook.com>

//! `adapter!` in modules that call another derive `Clone`, `Copy` or
//! `Debug`, or another attribute `derive`, through a `use ... as` of their
//! own.  A derive's path resolves
//! where the macro is called, as a type's does, so an expansion that
//! derived `Clone`, `Copy` and `Debug` by those names would derive the
//! caller's `Default` here, which an uninhabited adapter cannot implement.
//! An adapter derives `core`'s `Clone`, `Copy` and `Debug`, through
//! `core`'s `derive`, whatever those names mean beside the call, and each
//! module's own types and tests take what the module calls so.

#![forbid(unsafe_code)]

use tailq::{Link, Payload, TailQueue};

/// A node whose key is its payload.
pub struct Node {
    link: Link,
    key: u32,
}

/// A module that calls the `Default` derive `Debug`.
mod debug_is_default {
    use core::default::Default as Debug;

    /// The module's settings, which derive `Default`.
    #[derive(Debug)]
    pub struct Settings {
        pub key: u32,
    }

    tailq::adapter!(pub NodeLink = super::Node { link, mut key: u32 });
}

/// A module that calls the `Default` derive `Clone`.
mod clone_is_default {
    use core::default::Default as Clone;

    /// The module's settings, which derive `Default`.
    #[derive(Clone)]
    pub struct Settings {
        pub key: u32,
    }

    tailq::adapter!(pub NodeLink = super::Node { link, mut key: u32 });
}

/// A module that calls the `Default` derive `Copy`.
mod copy_is_default {
    use core::default::Default as Copy;

    /// The module's settings, which derive `Default`.
    #[derive(Copy)]
    pub struct Settings {
        pub key: u32,
    }

    tailq::adapter!(pub NodeLink = super::Node { link, mut key: u32 });
}

/// Requires what an adapter derives.
const fn derived<A>()
where
    A: Clone + Copy + core::fmt::Debug,
{
}

const _: () = derived::<debug_is_default::NodeLink>();
const _: () = derived::<clone_is_default::NodeLink>();
const _: () = derived::<copy_is_default::NodeLink>();

/// Links a node keyed `key` through `A`, bumps the key through its
/// payload and returns it.
fn bumped<A>(key: u32) -> u32
where
    A: Payload<Node = Node, Payload = u32>,
{
    let mut node = Node {
        link: Link::new(),
        key,
    };
    let mut queue = TailQueue::<'_, A>::new();
    queue.push_back(&mut node);
    if let Some(key) = queue.front_mut() {
        *key = key.wrapping_add(1);
    }
    queue.front().map_or(0, |front| front.key)
}

#[test]
fn adapters_derive_cores_clone_copy_and_debug_whatever_the_names_mean() {
    assert_eq!(
        bumped::<debug_is_default::NodeLink>(
            debug_is_default::Settings::default().key
        ),
        1
    );
    assert_eq!(
        bumped::<clone_is_default::NodeLink>(
            clone_is_default::Settings::default().key
        ),
        1
    );
    assert_eq!(
        bumped::<copy_is_default::NodeLink>(
            copy_is_default::Settings::default().key
        ),
        1
    );
}

/// A module whose `derive` is the `test` attribute.
mod tests_by_another_name {
    use core::prelude::v1::test as derive;
    use tailq::{Link, TailQueue};

    /// A node whose key is its payload.
    pub struct Node {
        link: Link,
        key: u32,
    }

    tailq::adapter!(pub NodeLink = Node { link, mut key: u32 });

    #[derive]
    fn adapters_derive_through_cores_derive_whatever_the_name_means() {
        let mut node = Node {
            link: Link::new(),
            key: 1,
        };
        let mut queue = TailQueue::<'_, NodeLink>::new();
        queue.push_back(&mut node);
        if let Some(key) = queue.front_mut() {
            *key = key.wrapping_add(1);
        }
        assert_eq!(queue.front().map(|front| front.key), Some(2));
    }
}
