//! Domain model, FIFO engine, reports, importers, FX rate resolution and storage.
//!
//! No HTTP and no terminal: see `design/architecture.md` [ARC-002].

pub mod allocation;
pub mod attribution;
pub mod decimal;
pub mod ecb;
pub mod effective_quantity;
pub mod entities;
pub mod fifo;
pub mod fx;
pub mod identity;
pub mod import;
pub mod income_tax;
pub mod manual_entry;
pub mod ordering;
pub mod precision;
pub mod quotation;
pub mod storage;
pub mod transaction;
pub mod transfer;
pub mod valuation;
