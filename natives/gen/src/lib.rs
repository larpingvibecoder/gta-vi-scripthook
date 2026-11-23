//! The natives database tool: `natives.tsv` reading and checking ([`db`]), code generation
//! ([`gen`]), importing public documentation and merging identifiers ([`import`]), and checking
//! a database's table locations against the owner's executable ([`verify`]).

pub mod db;
pub mod gen;
pub mod import;
pub mod pe;
pub mod verify;
