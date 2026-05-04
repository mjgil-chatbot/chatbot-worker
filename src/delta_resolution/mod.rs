//! Delta Resolution — external-memory git delta resolution without unbounded in-memory graphs.
//!
//! Implements Task 07 from the git-bounded design: pass-based delta resolution
//! using spill files, bounded memory windows, and streaming delta application.

mod delta;
