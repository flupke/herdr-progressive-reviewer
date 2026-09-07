use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) enum LanguageServer {
    RustAnalyzer,
    Expert,
    TypeScript,
}

impl LanguageServer {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::TypeScript => "TypeScript language server",
            _ => self.command(),
        }
    }

    pub(super) fn command(self) -> &'static str {
        match self {
            Self::RustAnalyzer => "rust-analyzer",
            Self::Expert => "expert",
            Self::TypeScript => "/bin/sh",
        }
    }

    pub(super) fn arguments(self) -> &'static [&'static str] {
        match self {
            Self::RustAnalyzer => &[],
            Self::Expert => &["--stdio"],
            // Select inside the loaded environment, then replace the shell so
            // the session owns the server process and its protocol streams.
            Self::TypeScript => &[
                "-c",
                "if command -v tsgo >/dev/null 2>&1; then exec tsgo --lsp --stdio; \
                 elif command -v typescript-language-server >/dev/null 2>&1; then exec typescript-language-server --stdio; \
                 else echo 'Install tsgo or typescript-language-server in the project environment' >&2; exit 127; fi",
            ],
        }
    }

    pub(super) fn initialization_options(self) -> Option<serde_json::Value> {
        (self == Self::TypeScript).then(|| {
            // The syntax server can return import aliases before project loading finishes.
            serde_json::json!({ "tsserver": { "useSyntaxServer": "never" } })
        })
    }

    fn root_markers(self) -> &'static [&'static str] {
        match self {
            Self::RustAnalyzer => &["Cargo.toml"],
            Self::Expert => &["mix.exs"],
            Self::TypeScript => &["tsconfig.json", "jsconfig.json", "package.json"],
        }
    }
}

pub(super) struct Language {
    pub(super) id: &'static str,
    pub(super) server: LanguageServer,
}

impl Language {
    pub(super) fn for_path(path: &Path) -> Option<Self> {
        let extension = path.extension()?.to_str()?.to_ascii_lowercase();
        let (id, server) = match extension.as_str() {
            "rs" => ("rust", LanguageServer::RustAnalyzer),
            "ex" | "exs" => ("elixir", LanguageServer::Expert),
            "eex" => ("eelixir", LanguageServer::Expert),
            "heex" => ("heex", LanguageServer::Expert),
            "ts" | "mts" | "cts" => ("typescript", LanguageServer::TypeScript),
            "tsx" => ("typescriptreact", LanguageServer::TypeScript),
            "js" | "mjs" | "cjs" => ("javascript", LanguageServer::TypeScript),
            "jsx" => ("javascriptreact", LanguageServer::TypeScript),
            _ => return None,
        };
        Some(Self { id, server })
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(super) struct Project {
    pub(super) server: LanguageServer,
    pub(super) root: PathBuf,
}

impl Project {
    pub(super) fn for_document(repository: &Path, path: &Path) -> Option<Self> {
        let server = Language::for_path(path)?.server;
        let root = path
            .parent()?
            .ancestors()
            .take_while(|ancestor| ancestor.starts_with(repository))
            .filter(|ancestor| {
                server
                    .root_markers()
                    .iter()
                    .any(|marker| ancestor.join(marker).is_file())
            })
            .last()
            .unwrap_or(repository)
            .to_owned();
        Some(Self { server, root })
    }
}

#[cfg(test)]
#[path = "language.tests.rs"]
mod tests;
