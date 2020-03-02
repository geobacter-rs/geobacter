
use super::*;
use geobacter_core::kernel::OptionalFn;

pub struct HostKillDetail;
impl PlatformImplDetail for HostKillDetail {
  fn kernel_instance() -> KernelInstance {
    fn host_kill() -> ! {
      panic!("Host workitem kill called");
    }

    host_kill.kernel_instance().unwrap()
  }
}
pub type WorkItemHostKill = WorkItemKill<HostKillDetail>;
