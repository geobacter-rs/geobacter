
//! Host platform utilities. This module is perhaps (now) poorly named.

#[path = "unix.rs"]
#[cfg(unix)]
pub mod os;

