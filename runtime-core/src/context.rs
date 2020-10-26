
use std::any::Any;
use std::collections::hash_map::{Entry, };
use std::error::Error;
use std::fmt::Debug;
use std::intrinsics::likely;
use std::sync::{Arc, Weak, atomic::AtomicUsize, atomic::Ordering, };

use indexvec::{Idx, IndexVec};
use parking_lot::{Mutex, RwLock, RwLockUpgradableReadGuard, MappedRwLockReadGuard,
                  RwLockReadGuard, RwLockWriteGuard, };

use rustc_span::SessionGlobals;
use rustc_data_structures::rayon::ThreadPoolBuilder;

use crate::{Accelerator, AcceleratorId, AcceleratorTargetDesc, Device};
use crate::codegen::{PlatformCodegen, CodegenDriver, PKernelDesc};
use crate::metadata::{context_metadata, LoadedCrateMetadata, };
use crate::utils::{HashMap, };

pub use rustc_session::config::OutputType;

type Translators = HashMap<
  Arc<AcceleratorTargetDesc>,
  Weak<dyn Any + Send + Sync + 'static>,
>;

type MappedReadResult<'a, T> = Result<
  MappedRwLockReadGuard<'a, T>,
  Box<dyn Error + Send + Sync + 'static>,
>;
type LoadedMetadataResult<'a> = MappedReadResult<'a, LoadedCrateMetadata>;
#[derive(Default)]
struct AsyncCodegenMetadataLoader(RwLock<Option<LoadedCrateMetadata>>);
impl AsyncCodegenMetadataLoader {
  fn load(&self) -> LoadedMetadataResult {
    {
      let r = self.0.read();
      if likely(r.is_some()) {
        return Ok(RwLockReadGuard::map(r, |r| {
          r.as_ref().unwrap()
        }));
      }
    }

    let mut w = self.0.write();
    if likely(w.is_none()) {
      // nobody beat us.
      *w = Some(context_metadata()?);
    }

    let r = RwLockWriteGuard::downgrade(w);
    return Ok(RwLockReadGuard::map(r, |r| {
      r.as_ref().unwrap()
    }));
  }
}

/// This structure should be used like you'd use a singleton.
struct ContextData {
  #[allow(dead_code)]
  session_globals: Arc<SessionGlobals>,
  metadata: AsyncCodegenMetadataLoader,

  next_accel_id: AtomicUsize,

  m: RwLock<ContextDataMut>,
}
/// Data that will be wrapped in a rw mutex.
struct ContextDataMut {
  accelerators: IndexVec<AcceleratorId, Option<Arc<dyn Accelerator>>>,

  translators: Translators,
}

#[derive(Clone)]
pub struct Context(Arc<ContextData>);

unsafe impl Send for Context { }
unsafe impl Sync for Context { }

static mut CTX: Option<WeakContext> = None;

impl Context {
  pub fn global() -> Option<Context> {
    // CTX can only be written during context initialization, which will run an acquire fence.
    // Otherwise this races, but that's NBD because this will just return None in that case
    unsafe {
      if let Some(ctx) = CTX.clone() {
        if let Some(ctx) = ctx.upgrade() {
          return Some(ctx);
        }
      }
    }
    None
  }

  pub fn new() -> Result<Context, Box<dyn Error>> {
    use crate::rustc_span::edition::Edition;

    if let Some(ctx) = Self::global() {
      return Ok(ctx);
    }

    lazy_static::lazy_static! {
      static ref INIT: Mutex<()> = parking_lot::const_mutex(());
    }
    let _ = INIT.lock(); // ensure only one of us is created
    if let Some(ctx) = Self::global() {
      return Ok(ctx);
    }

    crate::utils::env::initialize();

    crate::rustc_driver::init_rustc_env_logger();

    let session_globals = rustc_span::SessionGlobals::new(Edition::Edition2018);
    let session_globals = Arc::new(session_globals);
    let pool_globals = session_globals.clone();

    ThreadPoolBuilder::new()
      // give us a huge stack (for codegen's use):
      .stack_size(32 * 1024 * 1024)
      .thread_name(|id| format!("grt-core-worker-{}", id) )
      .deadlock_handler(|| unsafe { crate::rustc_middle::ty::query::handle_deadlock() })
      .spawn_handler(move |tb| {
        let mut b = std::thread::Builder::new();
        if let Some(name) = tb.name() {
          b = b.name(name.to_owned());
        }
        if let Some(stack_size) = tb.stack_size() {
          b = b.stack_size(stack_size);
        }
        let pool_globals = pool_globals.clone();
        b.spawn(move || {
          rustc_span::SESSION_GLOBALS.set(&*pool_globals, move || {
            tb.run()
          });
        })?;

        Ok(())
      })
      .build_global()?;

    let accelerators = IndexVec::new();
    let translators: Translators = Default::default();

    let data = ContextDataMut {
      accelerators,
      translators,
    };
    let data = ContextData {
      session_globals,
      metadata: AsyncCodegenMetadataLoader::default(),

      next_accel_id: AtomicUsize::new(0),

      m: RwLock::new(data),
    };
    let data = Arc::new(data);
    let context = Context(data);

    // now install the global ref:
    let weak = context.downgrade_ref();
    unsafe {
      CTX = Some(weak);
    }

    Ok(context)
  }

  pub(crate) fn load_metadata(&self) -> LoadedMetadataResult {
    self.0.metadata.load()
  }

  #[doc(hidden)]
  #[inline(always)]
  pub fn with_rustc_span_globals<F, R>(&self, f: F) -> R
    where F: FnOnce() -> R + Send,
          R: Send,
  {
    use rustc_span::SESSION_GLOBALS;

    let mut out = None;
    rustc_data_structures::rayon::scope(|s| {
      s.spawn(|_| {
        let o = SESSION_GLOBALS.set(&*self.0.session_globals, f);
        out = Some(o);
      });
    });
    out.unwrap()
  }

  pub fn downgrade_ref(&self) -> WeakContext {
    WeakContext(Arc::downgrade(&self.0))
  }

  pub fn filter_accels<F>(&self, f: F) -> Result<Vec<Arc<dyn Accelerator>>, Box<dyn Error>>
    where F: FnMut(&&Arc<dyn Accelerator>) -> bool,
  {
    let b = self.0.m.read();
    let r = b.accelerators.iter()
      .filter_map(|a| a.as_ref() )
      .filter(f)
      .cloned()
      .collect();
    Ok(r)
  }
  pub fn find_accel<F>(&self, f: F) -> Result<Option<Arc<dyn Accelerator>>, Box<dyn Error>>
    where F: FnMut(&&Arc<dyn Accelerator>) -> bool,
  {
    let b = self.0.m.read();
    let r = b.accelerators.iter()
      .filter_map(|a| a.as_ref() )
      .find(f)
      .map(|accel| accel.clone() );

    Ok(r)
  }

  pub fn get_accel_ref(&self, id: AcceleratorId)
                       -> Option<MappedRwLockReadGuard<dyn Accelerator>>
  {
    RwLockReadGuard::try_map(self.0.m.read(), |b| {
      b.accelerators.get(id)
        .and_then(|t| t.as_ref() )
        .map(|accel| &**accel)
    }).ok()
  }
  pub fn get_dev_ref<T>(&self, id: AcceleratorId)
                        -> Option<MappedRwLockReadGuard<T>>
    where T: Device,
  {
    let b = self.get_accel_ref(id)?;
    MappedRwLockReadGuard::try_map(b, |accel| {
      T::downcast_ref(accel)
    })
      .ok()
  }

  pub fn take_accel_id(&self) -> AcceleratorId {
    let id = self.0.next_accel_id
      .fetch_add(1, Ordering::AcqRel);
    if id > usize::max_value() / 2 {
      panic!("too many accelerators");
    }
    AcceleratorId::new(id)
  }

  /// Internal. User code most probably *shouldn't* use this function.
  #[doc = "hidden"]
  pub fn initialize_accel<T>(&self, accel: &mut Arc<T>)
    -> Result<(), Box<dyn Error + Send + Sync + 'static>>
    where T: Device,
  {
    debug_assert!(!Arc::get_mut(accel).is_none(),
                  "improper Context::initialize_accel usage");

    let target_desc = accel.accel_target_desc().clone();

    let mut w = self.0.m.write();
    match w.translators.entry(target_desc) {
      Entry::Occupied(mut o) => {
        Arc::get_mut(accel).unwrap()
          .set_accel_target_desc(o.key().clone());
        if let Some(cg) = o.get().upgrade() {
          Accelerator::set_target_codegen(accel, cg);
        } else {
          let cg = Accelerator::create_target_codegen(accel,
                                                      self)?;
          *o.get_mut() = Arc::downgrade(&cg);
        }
      },
      Entry::Vacant(v) => {
        let cg = Accelerator::create_target_codegen(accel, self)?;
        v.insert(Arc::downgrade(&cg));
      },
    }

    if w.accelerators.len() <= accel.id().index() {
      w.accelerators.resize(accel.id().index() + 1, None);
    }
    w.accelerators[accel.id()] = Some(accel.clone());

    Ok(())
  }
}

impl ContextDataMut { }
impl Eq for Context { }
impl PartialEq for Context {
  fn eq(&self, rhs: &Self) -> bool {
    Arc::ptr_eq(&self.0, &rhs.0)
  }
}
impl<'a> PartialEq<&'a Context> for Context {
  fn eq(&self, rhs: &&Self) -> bool {
    Arc::ptr_eq(&self.0, &rhs.0)
  }
}

#[derive(Clone)]
pub struct WeakContext(Weak<ContextData>);

unsafe impl Send for WeakContext { }
unsafe impl Sync for WeakContext { }

impl WeakContext {
  pub fn new() -> Self {
    WeakContext(Weak::new())
  }
  pub fn upgrade(&self) -> Option<Context> {
    self.0.upgrade()
      .map(|v| Context(v) )
  }
}

/// Platform and device specific module Stuff. Put your API handles
/// here!
pub trait PlatformModuleData: Any + Debug + Send + Sync + 'static {
  /// Sadly, `Eq` isn't trait object safe, so this must be
  /// implemented manually:
  /// ```
  /// # use std::sync::Arc;
  /// # use geobacter_runtime_core::context::*;
  /// # #[derive(Debug, Eq, PartialEq)]
  /// # struct MyPlatformModuleData(bool);
  /// # impl PlatformModuleData for MyPlatformModuleData {
  ///     fn eq(&self, rhs: &dyn PlatformModuleData) -> bool {
  ///       let rhs: Option<&Self> = Self::downcast_ref(rhs);
  ///       if let Some(rhs) = rhs {
  ///         self == rhs
  ///       } else {
  ///         false
  ///       }
  ///     }
  /// # }
  /// # let lhs = Arc::new(MyPlatformModuleData(true)) as Arc<dyn PlatformModuleData>;
  /// # let rhs = Arc::new(MyPlatformModuleData(true)) as Arc<dyn PlatformModuleData>;
  /// # assert!(PlatformModuleData::eq(&*lhs, &*rhs));
  /// # let rhs = Arc::new(MyPlatformModuleData(false)) as Arc<dyn PlatformModuleData>;
  /// # assert!(!PlatformModuleData::eq(&*lhs, &*rhs));
  /// ```
  fn eq(&self, rhs: &dyn PlatformModuleData) -> bool;

  /// Special downcast helper as trait objects can't be "reunsized" into
  /// another trait object, even when this trait requires
  /// `Self: Any + 'static`.
  fn downcast_ref(this: &dyn PlatformModuleData) -> Option<&Self>
    where Self: Sized,
  {
    use std::any::TypeId;
    use std::mem::transmute;
    use std::raw::TraitObject;

    let this_tyid = Any::type_id(this);
    let self_tyid = TypeId::of::<Self>();
    if this_tyid != self_tyid {
      return None;
    }

    // We have to do this manually.
    let this: TraitObject = unsafe { transmute(this) };
    let this = this.data as *mut Self;
    Some(unsafe { &*this })
  }

  /// Special downcast helper as trait objects can't be "reunsized" into
  /// another trait object, even when this trait requires
  /// `Self: Any + 'static`.
  fn downcast_arc(this: &Arc<dyn PlatformModuleData>) -> Option<Arc<Self>>
    where Self: Sized,
  {
    use std::any::TypeId;
    use std::mem::transmute;
    use std::raw::TraitObject;

    let this_tyid = Any::type_id(&**this);
    let self_tyid = TypeId::of::<Self>();
    if this_tyid != self_tyid {
      return None;
    }

    // We have to do this manually.
    let this = this.clone();
    let this = Arc::into_raw(this);
    let this: TraitObject = unsafe { transmute(this) };
    let this = this.data as *mut Self;
    Some(unsafe { Arc::from_raw(this) })
  }
}

pub struct ModuleData {
  ctxt: WeakContext,
  /// TODO use weak here and force the accelerator object store the
  /// strong reference.
  entries: RwLock<IndexVec<AcceleratorId, Option<Arc<dyn PlatformModuleData>>>>,
}
impl ModuleData {
  fn new(ctxt: &Context) -> ModuleData {
    ModuleData {
      ctxt: ctxt.downgrade_ref(),
      entries: Default::default(),
    }
  }
  fn get<D>(&self, accel_id: AcceleratorId,
            expect_platform_ty: bool) -> Option<Arc<D::ModuleData>>
    where D: Device,
  {
    let read = self.entries.read();
    read.get(accel_id)
      .and_then(|v| v.as_ref() )
      .and_then(|v| {
        <D::ModuleData as PlatformModuleData>::downcast_arc(v)
          // emit a warning if this object doesn't have the type we expect:
          .or_else(|| {
            if expect_platform_ty {
              panic!("unexpected platform module type in accelerator slot!");
            } else {
              warn!("unexpected platform module type in accelerator slot: {:#?}", v);
            }
            None
          })
      })
  }
  pub fn compile<D, P>(&self,
                       accel: &Arc<D>,
                       desc: PKernelDesc<P>,
                       codegen: &CodegenDriver<P>,
                       expect_platform_ty: bool)
    -> Result<Arc<D::ModuleData>, D::Error>
    where D: Device<Codegen = P>,
          P: PlatformCodegen<Device = D>,
  {
    let accel_id = accel.id();
    if let Some(entry) = self.get::<D>(accel_id, expect_platform_ty) {
      return Ok(entry);
    }

    // serialize the rest of this function, but still allow normal reads
    // to get existing entries.
    let guard = self.entries.upgradable_read();

    if let Some(ref prev) = guard.get(accel_id).and_then(|v| v.as_ref() ) {
      let prev = <D::ModuleData as PlatformModuleData>::downcast_arc(prev);
      if let Some(module) = prev {
        // someone beat us, don't create another platform module object
        return Ok(module);
      } else {
        // ??? what?
        if expect_platform_ty {
          panic!("unexpected platform module type in accelerator slot!");
        }
      }
    }

    let codegen = codegen.codegen(desc)?;

    // upgrade the read to a write
    let mut guard = RwLockUpgradableReadGuard::upgrade(guard);
    if guard.len() <= accel_id.index() {
      guard.resize(accel_id.index() + 1, None);
    }

    let module = D::load_kernel(accel, &*codegen)?;
    guard[accel_id] = Some(module.clone());
    return Ok(module);
  }
}
#[derive(Clone, Copy, Debug)]
/// No PhantomData on this, this object doesn't own the arguments or return
/// values of the function it represents.
/// Internal.
pub struct ModuleContextData(&'static AtomicUsize);

impl ModuleContextData {
  pub fn upgrade(&self, context: &Context) -> Option<Arc<ModuleData>> {
    let ptr_usize = self.0.load(Ordering::Acquire);
    if ptr_usize == 0 { return None; }
    let ptr = ptr_usize as *const ModuleData;

    let arc = unsafe { Arc::from_raw(ptr) };
    let arc_clone = arc.clone();
    // don't downref the global arc:
    Arc::into_raw(arc);

    let arc = arc_clone;
    let expected_context = arc.ctxt.upgrade();
    if expected_context.is_none() { return None; }
    let expected_context = expected_context.unwrap();
    assert!(expected_context == context,
            "there are two context's live at the same time");

    Some(arc)
  }

  /// Drops all globally stored data. This doesn't clear any
  /// data stored with any context.
  pub fn drop(&self) {
    let ptr_usize = self.0.load(Ordering::Acquire);
    if ptr_usize == 0 { return; }

    let owner = self.0
      .compare_exchange(ptr_usize, 0,
                        Ordering::SeqCst,
                        Ordering::Relaxed);
    if let Ok(_) = owner {
      let ptr = ptr_usize as *const ModuleData;
      unsafe { Arc::from_raw(ptr) };
    }
  }

  pub fn is_none(&self) -> bool {
    !self.is_some()
  }
  pub fn is_some(&self) -> bool {
    self.0.load(Ordering::Acquire) != 0
  }

  pub fn get_cache_data(&self, context: &Context)
    -> Arc<ModuleData>
  {
    use std::intrinsics::unlikely;

    let mut cached = self.upgrade(context);
    if unlikely(cached.is_none()) {
      let data = ModuleData::new(context);
      let data = Arc::new(data);
      let data_ptr = Arc::into_raw(data.clone());
      let data_usize = data_ptr as usize;

      // conservative orderings b/c this isn't the fast path.
      let actual_data_usize = self.0
        .compare_exchange(0, data_usize,
                          Ordering::SeqCst,
                          Ordering::SeqCst);
      match actual_data_usize {
        Ok(0) => {
          cached = Some(data);
        },
        Ok(_) => unreachable!(),
        Err(actual_data_usize) => {
          // either someone beat us, or the data is from an old context.
          let cached2 = self.upgrade(context);
          if cached2.is_some() {
            // someone beat us.
            unsafe { Arc::from_raw(data_ptr) };
            cached = cached2;
          } else {
            // the data is old. we need to clean up, while allowing for
            // possible races in this process.

            let r = self.0
              .compare_exchange(actual_data_usize,
                                data_usize,
                                Ordering::SeqCst,
                                Ordering::SeqCst);
            match r {
              Ok(actual_data_usize) => {
                // do the cleanup:
                let actual_data = actual_data_usize as *const ModuleData;
                let _actual_data = unsafe { Arc::from_raw(actual_data) };
                // let actual_data drop.

                cached = Some(data);
              },
              Err(_) => {
                // someone beat us.
                unsafe { Arc::from_raw(data_ptr) };
                let data = self.upgrade(context)
                  .expect("someone beat us in setting context \
                           module data, but didn't set it to \
                           value data");
                cached = Some(data);
              },
            }
          }
        },
      }
    }

    cached.unwrap()
  }

  pub fn get<F, Args, Ret>(_: &F) -> Self
    where F: Fn<Args, Output = Ret>,
  {
    use std::geobacter::intrinsics::geobacter_kernel_codegen_stash;
    ModuleContextData(unsafe {
      let data_ref = geobacter_kernel_codegen_stash::<F, _, _>();
      std::mem::transmute(data_ref)
    })
  }
}
impl Eq for ModuleContextData { }
impl PartialEq for ModuleContextData {
  fn eq(&self, rhs: &Self) -> bool {
    self.0 as *const AtomicUsize == rhs.0 as *const AtomicUsize
  }
}
impl<'a> PartialEq<&'a ModuleContextData> for ModuleContextData {
  fn eq(&self, rhs: &&Self) -> bool {
    self.0 as *const AtomicUsize == rhs.0 as *const AtomicUsize
  }
}

#[cfg(test)]
mod test {
  use super::*;
  use crate::utils::test::*;

  #[derive(Debug)]
  struct MyPlatformModuleData;

  impl PlatformModuleData for MyPlatformModuleData {
    fn eq(&self, _: &dyn PlatformModuleData) -> bool {
      unimplemented!()
    }
  }
  #[test]
  fn module_data_downcast() {
    let arc = Arc::new(MyPlatformModuleData) as Arc<dyn PlatformModuleData>;
    assert!(MyPlatformModuleData::downcast_arc(&arc).is_some());
    assert!(MyPlatformModuleData::downcast_ref(&*arc).is_some());
  }

  #[test]
  fn function_module_data_drop() {
    fn f() { }
    let data = ModuleContextData::get(&f);
    assert!(data.is_none());
    data.get_cache_data(context());

    assert!(data.is_some());
    data.drop();
    assert!(data.is_none());
  }
}
