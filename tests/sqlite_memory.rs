#[path = "sqlite_memory/contract.rs"]
mod contract;
#[path = "sqlite_memory/schema.rs"]
mod schema;
#[path = "sqlite_memory/search.rs"]
mod search;

use asteroniris::memory::SqliteMemory;
use tempfile::TempDir;

pub(crate) fn temp_sqlite() -> (TempDir, SqliteMemory) {
    let tmp = TempDir::new().expect("tempdir");
    let mem = SqliteMemory::new(tmp.path()).expect("sqlite memory");
    (tmp, mem)
}
