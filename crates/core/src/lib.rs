//! Backend do Orquestrador sem Tauri: terminais em que um CLI orquestra e outros executam.
pub mod agents;
pub mod clis;
pub mod config;
pub mod core;
pub mod ide;
pub mod open_paths;
pub mod platform;
pub mod proc;
#[cfg(test)]
mod testutil;
