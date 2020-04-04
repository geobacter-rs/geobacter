use super::*;
use crate::signal::deps::CompletionDep;

pub trait Completion {
  type CompletionSignal: SignalHandle + ?Sized;

  fn completion(&self) -> &Self::CompletionSignal;

  #[inline(always)]
  fn into_dep(self) -> CompletionDep<Self>
    where Self: Sized,
  {
    CompletionDep::new(self)
  }
}
