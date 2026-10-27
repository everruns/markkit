//! Pluggable format parsers.
//!
//! Decisions:
//! - Embedders register [`FormatParser`]s on the builder
//!   ([`crate::MarkkitBuilder::parser`]). For each document, registered
//!   parsers are consulted before the built-ins, in registration order:
//!   first one whose [`FormatParser::claims`] returns true, else the first
//!   one listing the detected [`Format`] in [`FormatParser::formats`].
//!   This lets an embedder add a format (PDF, EPUB, ...) or replace a
//!   built-in without forking.
//! - A custom parser gets the same guarantees as built-ins: input is
//!   size-checked first, a panic becomes [`crate::Error::Parser`], transforms
//!   run on its output, and the shared renderer produces the Markdown. Its own
//!   resource limits are its responsibility (it sees the [`MarkkitBuilder`]).
//! - Parsers are `Arc`-shared so the builder stays `Clone`.

use std::fmt;
use std::sync::Arc;

use crate::config::MarkkitBuilder;
use crate::extract::ParsedDocument;
use crate::format::{DocumentHint, Format};

/// A parser from bytes to the IR.
pub trait FormatParser: Send + Sync {
    /// Short name for debugging and error messages.
    fn name(&self) -> &str;

    /// Detected formats this parser handles (see [`Format::detect`]).
    fn formats(&self) -> &[Format] {
        &[]
    }

    /// Claim input before format detection (for formats [`Format`] has no
    /// variant for). Inputs are untrusted; keep this cheap.
    fn claims(&self, _bytes: &[u8], _hint: DocumentHint<'_>) -> bool {
        false
    }

    /// Parse `bytes` (already within `config.max_input_size`) into the IR.
    fn parse(&self, bytes: &[u8], config: &MarkkitBuilder) -> crate::Result<ParsedDocument>;
}

/// Ordered list of custom parsers held by [`MarkkitBuilder`].
#[derive(Clone, Default)]
pub struct ParserRegistry(Vec<Arc<dyn FormatParser>>);

impl ParserRegistry {
    /// Register a parser (lower priority than those registered earlier).
    pub fn push(&mut self, p: impl FormatParser + 'static) {
        self.0.push(Arc::new(p));
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Names of the parsers, in priority order.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(|p| p.name())
    }

    /// Parser that claims the input, else the first one for `detected`.
    pub(crate) fn select(
        &self,
        bytes: &[u8],
        hint: DocumentHint<'_>,
        detected: Format,
    ) -> Option<&dyn FormatParser> {
        self.0
            .iter()
            .find(|p| p.claims(bytes, hint))
            .or_else(|| self.0.iter().find(|p| p.formats().contains(&detected)))
            .map(|p| p.as_ref())
    }
}

impl fmt::Debug for ParserRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.names()).finish()
    }
}
