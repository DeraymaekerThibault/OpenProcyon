//! Binary: runs the shared game library (`lib.rs`) with the classic (retail) presentation.
//! Other presentations can live in their own crates and call `openprocyon::run` with their own view plugin (docs/development.md "Layers").

mod classic;

// The view addresses shared code as `crate::menu`, `crate::panels`, ...
pub use openprocyon::*;

fn main() {
    openprocyon::run(classic::plugin, classic::setup::plugin);
}
