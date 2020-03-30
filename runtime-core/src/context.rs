
use std::any::Any;
use std::collections::hash_map::{Entry, };
use std::error::Error;
use std::fmt::Debug;
use std::intrinsics::likely;
use std::sync::{Arc, Weak, atomic::AtomicUsize, atomic::Ordering, };

use indexvec::{Idx, IndexVec};
use parking_lot::{RwLock, RwLockUpgradableReadGuard, MappedRwLockReadGuard,
                  RwLockReadGuard, RwLockWriteGuard, };

use crate::rustc_data_structures::rayon::ThreadPoolBuilder;

use crate::{Accelerator, AcceleratorId, AcceleratorTargetDesc, Device};
use crate::codegen::{PlatformCodegen, CodegenDriver, PKernelDesc};
use crate::metadata::{context_metadata, LoadedCrateMetadata, };
use crate::utils::{HashMap, };

pub use rustc::session::config::OutputType;

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
pub struct ContextData {
  #[allow(dead_code)]
  syntax_globals: Arc<syntax::attr::Globals>,
  metadata: AsyncCodegenMetadataLoader,

  next_accel_id: AtomicUsize,

  m: RwLock<ContextDataMut>,
}
/// Data that will be wrapped in a rw mutex.
pub struct ContextDataMut {
  accelerators: IndexVec<AcceleratorId, Option<Arc<dyn Accelerator>>>,

  translators: Translators,
}

#[derive(Clone)]
pub struct Context(Arc<ContextData>);

unsafe impl Send for Context { }
unsafe impl Sync for Context { }

impl Context {
  pub fn new() -> Result<Context, Box<dyn Error>> {
    use crate::rustc_span::edition::Edition;

    crate::utils::env::initialize();

    crate::rustc_driver::init_rustc_env_logger();

    let syntax_globals = syntax::attr::Globals::new(Edition::Edition2018);
    let syntax_globals = Arc::new(syntax_globals);
    let pool_globals = syntax_globals.clone();

    ThreadPoolBuilder::new()
      // give us a huge stack (for codegen's use):
      .stack_size(32 * 1024 * 1024)
      .thread_name(|id| format!("grt-core-worker-{}", id) )
      .deadlock_handler(|| unsafe { crate::rustc::ty::query::handle_deadlock() })
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
          pool_globals.with(|| {
            tb.run()
          })
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
      syntax_globals,
      metadata: AsyncCodegenMetadataLoader::default(),

      next_accel_id: AtomicUsize::new(0),

      m: RwLock::new(data),
    };
    let data = Arc::new(data);
    let context = Context(data);

    Ok(context)
  }

  pub(crate) fn load_metadata(&self) -> LoadedMetadataResult {
    self.0.metadata.load()
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
  pub fn upgrade(&self) -> Option<Context> {
    self.0.upgrade()
      .map(|v| Context(v) )
  }
}

/// Platform and device specific module Stuff. Put your API handles
/// here!
pub trait PlatformModuleData: Any + Debug + Send + Sync + 'static {
  /// Sadly, `Eq` isn't trait object safe, so this must be
  /// implemented manually.
  fn eq(&self, rhs: &dyn PlatformModuleData) -> bool;

  /// Special downcast helper as trait objects can't be "reunsized" into
  /// another trait object, even when this trait requires
  /// `Self: Any + 'static`.
  ///
  /// Must be implemented manually :(
  /// Just paste this into your impls:
  /// ```ignore
  /// fn downcast_ref(this: &dyn PlatformModuleData) -> Option<&Self>
  ///    where Self: Sized,
  /// {
  ///    use std::any::TypeId;
  ///    use std::mem::transmute;
  ///    use std::raw::TraitObject;
  ///
  ///    if this.type_id() != TypeId::of::<Self>() {
  ///      return None;
  ///    }
  ///
  ///    // We have to do this manually.
  ///    let this: TraitObject = unsafe { transmute(this) };
  ///    let this = this.data as *mut Self;
  ///    Some(unsafe { &*this })
  ///  }
  /// ```
  fn downcast_ref(this: &dyn PlatformModuleData) -> Option<&Self>
    where Self: Sized;

  /// Special downcast helper as trait objects can't be "reunsized" into
  /// another trait object, even when this trait requires
  /// `Self: Any + 'static`.
  ///
  /// Must be implemented manually :(
  /// Just paste this into your impls:
  /// ```ignore
  /// fn downcast_ref(this: &dyn PlatformModuleData) -> Option<&Self>
  ///    where Self: Sized,
  /// {
  ///    use std::any::TypeId;
  ///    use std::mem::transmute;
  ///    use std::raw::TraitObject;
  ///
  ///    if this.type_id() != TypeId::of::<Self>() {
  ///      return None;
  ///    }
  ///
  ///    // We have to do this manually.
  ///    let this = this.clone();
  ///    let this = Arc::into_raw(this);
  ///    let this: TraitObject = unsafe { transmute(this) };
  ///    let this = this.data as *mut Self;
  ///    Some(unsafe { Arc::from_raw(this) })
  ///  }
  /// ```
  fn downcast_arc(this: &Arc<dyn PlatformModuleData>) -> Option<Arc<Self>>
    where Self: Sized;
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
  fn get<D>(&self, accel_id: AcceleratorId) -> Option<Arc<D::ModuleData>>
    where D: Device,
  {
    let read = self.entries.read();
    read.get(accel_id)
      .and_then(|v| v.as_ref() )
      .and_then(|v| {
        <D::ModuleData as PlatformModuleData>::downcast_arc(v)
          // emit a warning if this object doesn't have the type we expect:
          .or_else(|| {
            warn!("unexpected platform module type in accelerator slot!");
            None
          })
      })
  }
  pub fn compile<D, P>(&self,
                       accel: &Arc<D>,
                       desc: PKernelDesc<P>,
                       codegen: &CodegenDriver<P>)
    -> Result<Arc<D::ModuleData>, D::Error>
    where D: Device<Codegen = P>,
          P: PlatformCodegen<Device = D>,
  {
    let accel_id = accel.id();
    if let Some(entry) = self.get::<D>(accel_id) {
      return Ok(entry);
    }

    // serialize the rest of this function, but still allow normal reads
    // to get existing entries.
    let guard = self.entries.upgradable_read();

    let codegen = codegen.codegen(desc)?;

    // upgrade the read to a write
    let mut guard = RwLockUpgradableReadGuard::upgrade(guard);
    if guard.len() <= accel_id.index() {
      guard.resize(accel_id.index() + 1, None);
    } else {
      if let Some(ref prev) = guard[accel_id] {
        let prev = <D::ModuleData as PlatformModuleData>::downcast_arc(prev);
        if let Some(module) = prev {
          // Don't create another platform module object
          return Ok(module);
        }
      }
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
    self.0.store(0, Ordering::Release);

    let ptr = ptr_usize as *const ModuleData;

    unsafe { Arc::from_raw(ptr) };
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

  pub fn get<F, Args, Ret>(f: &F) -> Self
    where F: Fn<Args, Output = Ret>,
  {
    use geobacter_core::kernel::kernel_context_data_id;
    let data_ref = kernel_context_data_id(f);
    ModuleContextData(data_ref)
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
