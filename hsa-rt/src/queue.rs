use std::error::Error;

use ffi;
use ApiContext;
use agent::Agent;

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Queue(ffi::hsa_queue_t, ApiContext);

impl Queue {
  pub fn new() { }
}

