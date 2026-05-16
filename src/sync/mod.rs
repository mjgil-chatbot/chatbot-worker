//! Cross-instance synchronization via shared memory.
//!
//! Uses mmap to share instance registry and session version state between
//! multiple instances of the application. On Linux the backing file lives
//! under `/dev/shm/`; on macOS it is created with `shm_open(3)` (POSIX
//! shared memory).
//!
//! Layout is defined by `#[repr(C)]` structs with zerocopy endian types
//! (alignment 1) so the on-disk format is portable and free of padding.

mod layout;
mod sync_shm;
mod util;
pub use layout::*;
pub use util::*;
#[cfg(test)]
mod tests;
