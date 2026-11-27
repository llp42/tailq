// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Leonardo Lopes Pereira <leonardolopespereira@outlook.com>

//! `flip!(key; T0, T1, ...)` expands to `T0` the first time it is expanded
//! with that key, `T1` the second, and so on, then the last one: a type
//! macro that is not a function of its input, written in safe code.  The
//! fifth review used one to show that `adapter!` pasted its node type
//! several times, so that each paste could name another type.

#![forbid(unsafe_code)]

use core::mem::take;
use proc_macro::{TokenStream, TokenTree};
use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

/// How many times each key has been expanded.
static COUNTS: Mutex<Option<HashMap<String, usize>>> = Mutex::new(None);

/// Returns how many times `key` was expanded before, and counts this one.
fn expansions(key: String) -> usize {
    let mut guard = COUNTS.lock().unwrap_or_else(PoisonError::into_inner);
    let count = guard
        .get_or_insert_with(HashMap::new)
        .entry(key)
        .or_insert(0);
    let before = *count;
    *count = count.saturating_add(1);
    drop(guard);
    before
}

/// Expands to the next of its types for its key.
#[proc_macro]
#[inline]
pub fn flip(input: TokenStream) -> TokenStream {
    let mut tokens = input.into_iter();
    let key = tokens
        .next()
        .map(|token| token.to_string())
        .unwrap_or_default();
    let mut choices: Vec<TokenStream> = Vec::new();
    let mut choice: Vec<TokenTree> = Vec::new();
    let mut depth: usize = 0;
    for token in tokens.skip(1) {
        if let TokenTree::Punct(punct) = token.clone() {
            match punct.as_char() {
                '<' => depth = depth.saturating_add(1),
                '>' => depth = depth.saturating_sub(1),
                ',' if depth == 0 => {
                    choices.push(take(&mut choice).into_iter().collect());
                    continue;
                }
                _ => {}
            }
        }
        choice.push(token);
    }
    if !choice.is_empty() {
        choices.push(choice.into_iter().collect());
    }
    let index = expansions(key).min(choices.len().saturating_sub(1));
    choices.into_iter().nth(index).unwrap_or_default()
}
