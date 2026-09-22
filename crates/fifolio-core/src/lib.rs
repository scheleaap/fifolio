//! Domain model, FIFO engine, reports, importers, FX rate resolution and storage.
//!
//! No HTTP and no terminal: see `design/architecture.md` [ARC-002].

pub mod decimal;
pub mod entities;
pub mod identity;
pub mod manual_entry;
pub mod ordering;
pub mod precision;
pub mod quotation;
pub mod transaction;
