#![feature(const_fn)]

#![allow(non_upper_case_globals, non_camel_case_types,
         non_snake_case, broken_intra_doc_links)]

use std::cmp::Ordering;

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

macro_rules! impl_handle_cmp {
  ($($ty:ty,)*) => {$(

    impl Eq for $ty { }
    impl PartialEq for $ty {
      fn eq(&self, rhs: &$ty) -> bool {
        self.handle == rhs.handle
      }
    }
    impl Ord for $ty {
      fn cmp(&self, rhs: &$ty) -> Ordering {
        self.handle.cmp(&rhs.handle)
      }
    }
    impl PartialOrd for $ty {
      fn partial_cmp(&self, rhs: &$ty) -> Option<Ordering> {
        self.handle.partial_cmp(&rhs.handle)
      }
    }
  )*};
}

impl_handle_cmp! {
  hsa_agent_s, hsa_amd_memory_pool_s, hsa_cache_s, hsa_code_object_reader_s,
  hsa_executable_s, hsa_executable_symbol_s, hsa_isa_s,
  hsa_loaded_code_object_s, hsa_region_s,
  hsa_signal_s, hsa_signal_group_s, hsa_wavefront_s,
}
