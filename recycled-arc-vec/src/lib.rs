
use std::sync::atomic::AtomicUsize;

struct Counter<T>
{
  count: AtomicUsize,
  data: T,
}
