//! Stateful matching of keys against one subscription's commands.

use std::marker::PhantomData;

use component_core::{InputMatcher, InputResolution};

use crate::Key;
use crate::commands::ShortcutSubscription;
use crate::table::bindings;

/// Stateful matching for one shortcut subscription.
///
/// The matcher only sees bindings whose command belongs to a scope that `S`
/// listens to, and resolves them to `S`.
pub struct ShortcutMatcher<S> {
    prefix: Option<Key>,
    subscription: PhantomData<fn() -> S>,
}

impl<S: ShortcutSubscription> Default for ShortcutMatcher<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: ShortcutSubscription> ShortcutMatcher<S> {
    pub const fn new() -> Self {
        Self {
            prefix: None,
            subscription: PhantomData,
        }
    }

    pub fn resolve_key(&mut self, key: Key) -> InputResolution<S> {
        let prefix = self.prefix.take();
        if let Some(command) = Self::matching_command(prefix, key) {
            return InputResolution::Matched(command);
        }
        if prefix.is_some()
            && let Some(command) = Self::matching_command(None, key)
        {
            return InputResolution::Matched(command);
        }
        if Self::starts_sequence(key) {
            self.prefix = Some(key);
            InputResolution::AwaitingMoreInput
        } else {
            InputResolution::NoMatch
        }
    }

    fn matching_command(prefix: Option<Key>, key: Key) -> Option<S> {
        bindings()
            .filter(|binding| binding.sequence.matches(prefix, key))
            .find_map(|binding| S::select(binding.command))
    }

    fn starts_sequence(key: Key) -> bool {
        bindings().any(|binding| {
            binding.sequence.starts_with(key) && S::select(binding.command).is_some()
        })
    }
}

impl<C, S: ShortcutSubscription> InputMatcher<C, Key> for ShortcutMatcher<S> {
    type Output = S;

    fn resolve(&mut self, _component: &C, key: &Key) -> InputResolution<Self::Output> {
        self.resolve_key(*key)
    }
}
