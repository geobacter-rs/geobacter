
pub trait Args {
  type CompletionSignal: Clone;
  fn completion(&self) -> Self::CompletionSignal;

}
