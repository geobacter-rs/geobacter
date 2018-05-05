use std::error::Error;

use ffi;
use agent::Agent;

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Queue(ffi::hsa_queue_t);

impl Queue {
  pub fn new() { }
}

