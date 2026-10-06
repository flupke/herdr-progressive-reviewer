//! The TypeScript declarations of the socket's messages, generated from their Rust types with
//! ts-rs: the client's modules name them in their `JSDoc`, and `tsc --checkJs` checks the client
//! against them (.agents/wiki/explore-page-client.md). The declarations are committed in
//! `assets/client/types.ts`; the test below fails when they no longer match the Rust types, and
//! `make explore-types` writes them again.

use std::collections::BTreeMap;

use ts_rs::{Config, TS, TypeVisitor};

use crate::rpc::{Call, Notification, Reply, Request};

/// The file the client's modules import the types from.
const FILE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/client/types.ts");

/// Every type of the socket's messages, each declared once, by name: the order ts-rs visits
/// them in changes from one build to the next.
fn declarations() -> String {
    let mut collected = Declarations {
        config: Config::new().with_large_int("number"),
        declared: BTreeMap::new(),
    };
    collected.visit::<Notification>();
    collected.visit::<Request>();
    collected.visit::<Call>();
    collected.visit::<Reply>();
    let mut lines = vec![
        "// The types of the messages of the Explore page's socket. Generated from the Rust types by \
         `make explore-types`: do not edit."
            .to_owned(),
    ];
    lines.extend(collected.declared.into_values());
    lines.push(String::new());
    lines.join("\n")
}

/// Declares each type it visits, then the types it depends on.
struct Declarations {
    config: Config,
    /// Each declaration, with its documentation, by the type's name.
    declared: BTreeMap<String, String>,
}

impl TypeVisitor for Declarations {
    fn visit<T: TS + 'static + ?Sized>(&mut self) {
        // Only the types that derive `TS` have a declaration; a wrapper (`Option`, `Vec`)
        // declares its contents.
        if T::output_path().is_some() {
            let name = T::ident(&self.config);
            if self.declared.contains_key(&name) {
                return;
            }
            let docs = T::docs()
                .map(|docs| format!("{}\n", docs.trim_end()))
                .unwrap_or_default();
            let declaration = format!("{docs}export {}", T::decl(&self.config));
            self.declared.insert(name, declaration);
        }
        T::visit_dependencies(self);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clients_types_are_those_of_the_socket_messages() {
        let generated = declarations();
        if std::env::var("EXPLORE_TYPES").as_deref() == Ok("write") {
            std::fs::write(FILE, &generated).unwrap();
            return;
        }
        let committed = std::fs::read_to_string(FILE).unwrap_or_default();
        assert!(
            committed == generated,
            "{FILE} does not match the Rust types of the socket's messages: run `make explore-types`"
        );
    }
}
