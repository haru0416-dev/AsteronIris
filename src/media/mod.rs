pub mod detection;
pub mod processing;
pub mod storage;
pub mod types;

pub use processing::MediaProcessor;
pub use storage::MediaStore;
pub use types::{MediaConfig, MediaFile, MediaType, StoredMedia};
