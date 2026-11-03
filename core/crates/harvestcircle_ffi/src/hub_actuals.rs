//! Explicit actual-fact transport. Reports, previews and immutable effects differ.
mod input;
mod output;
pub use input::*;
pub use output::*;

#[cfg(test)]
mod tests;
