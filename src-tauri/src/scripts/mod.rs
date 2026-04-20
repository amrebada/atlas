//! Script discovery, parsing, and (non-PTY) execution.

pub mod parser;
pub mod runner;

// Re-export the broadly-useful entry points. Sub-modules are still `pub` so
pub use parser::discover_scripts;
pub use runner::run;
