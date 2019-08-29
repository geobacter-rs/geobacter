
//! These macros match their respective libcore versions, but only
//! panic on the host.

/// This macro uses `unreachable!` on the host, but uses
/// `::std::intrinsics::abort` on accelerators.
#[macro_export]
macro_rules! host_unreachable {
  () => ({
    if $crate::platform::is_host() {
      unreachable!();
    } else {
      #[allow(unused_unsafe)]
      unsafe { $crate::intrinsics::__geobacter_kill() };
    }
  });
  ($msg:expr) => ({
    if $crate::platform::is_host() {
      unreachable!($msg);
    } else {
      #[allow(unused_unsafe)]
      unsafe { $crate::intrinsics::__geobacter_kill() };
    }
  });
  ($msg:expr,) => ({
    if $crate::platform::is_host() {
      unreachable!($msg,);
    } else {
      #[allow(unused_unsafe)]
      unsafe { $crate::intrinsics::__geobacter_kill() };
    }
  });
  ($fmt:expr, $($arg:tt)*) => ({
    if $crate::platform::is_host() {
      unreachable!($fmt, $($arg)*);
    } else {
      #[allow(unused_unsafe)]
      unsafe { $crate::intrinsics::__geobacter_kill() };
    }
  });
}
#[macro_export]
macro_rules! host_unimplemented {
  () => ({
    if $crate::platform::is_host() {
      unimplemented!();
    } else {
      #[allow(unused_unsafe)]
      unsafe { $crate::intrinsics::__geobacter_kill() };
    }
  });
  ($msg:expr) => ({
    if $crate::platform::is_host() {
      unimplemented!($msg);
    } else {
      #[allow(unused_unsafe)]
      unsafe { $crate::intrinsics::__geobacter_kill() };
    }
  });
  ($msg:expr,) => ({
    if $crate::platform::is_host() {
      unimplemented!($msg,);
    } else {
      #[allow(unused_unsafe)]
      unsafe { $crate::intrinsics::__geobacter_kill() };
    }
  });
  ($fmt:expr, $($arg:tt)*) => ({
    if $crate::platform::is_host() {
      unimplemented!($fmt, $($arg)*);
    } else {
      #[allow(unused_unsafe)]
      unsafe { $crate::intrinsics::__geobacter_kill() };
    }
  });
}
/// Always runs the condition, but will only assert on the host.
/// On accelerators it will abort the work item!
#[macro_export]
macro_rules! host_assert {
  ($cond:expr) => ({
    assert!($cond || {
      // $cond is false
      if !$crate::platform::is_host() {
        #[allow(unused_unsafe)]
        unsafe { $crate::intrinsics::__geobacter_kill() };
      }
      false
    });
  });
  ($cond:expr,) => ({
    assert!($cond || {
      // $cond is false
      if !$crate::platform::is_host() {
        #[allow(unused_unsafe)]
        unsafe { $crate::intrinsics::__geobacter_kill() };
      }
      false
    });
  });
  ($cond:expr, $($arg:tt)+) => ({
    assert!($cond || {
      // $cond is false
      if !$crate::platform::is_host() {
        // no printing :(
        #[allow(unused_unsafe)]
        unsafe { $crate::intrinsics::__geobacter_kill() };
      }
      false
    }, $($arg)+);
  });
}
/// Assert two values are equal, but only on the host. The values are always evaluated.
#[macro_export]
macro_rules! host_assert_eq {
  ($left:expr, $right:expr) => ({
    match (&$left, &$right) {
      (left_val, right_val) => {
        if !(*left_val == *right_val) {
          if $crate::platform::is_host() {
            // The reborrows below are intentional. Without them, the stack slot for the
            // borrow is initialized even before the values are compared, leading to a
            // noticeable slow down.
            panic!(r#"assertion failed: `(left == right)`
  left: `{:?}`,
 right: `{:?}`"#, &*left_val, &*right_val);
          } else {
            // no printing :(
            #[allow(unused_unsafe)]
            unsafe { $crate::intrinsics::__geobacter_kill() };
          }
        }
      }
    }
  });
  ($left:expr, $right:expr,) => ({
    $crate::host_assert_eq!($left, $right)
  });
  ($left:expr, $right:expr, $($arg:tt)+) => ({
    match (&$left, &$right) {
      (left_val, right_val) => {
        if !(*left_val == *right_val) {
          if $crate::platform::is_host() {
            // The reborrows below are intentional. Without them, the stack slot for the
            // borrow is initialized even before the values are compared, leading to a
            // noticeable slow down.
            panic!(r#"assertion failed: `(left == right)`
  left: `{:?}`,
 right: `{:?}`"#, &*left_val, &*right_val,
                   format_args!($($arg)+));
          } else {
            // no printing :(
            #[allow(unused_unsafe)]
            unsafe { $crate::intrinsics::__geobacter_kill() };
          }
        }
      }
    }
  });
}
/// Assert two values are not equal, but only on the host. The values are always evaluated.
#[macro_export]
macro_rules! host_assert_ne {
  ($left:expr, $right:expr) => ({
    match (&$left, &$right) {
      (left_val, right_val) => {
        if *left_val == *right_val {
          if $crate::platform::is_host() {
            // The reborrows below are intentional. Without them, the stack slot for the
            // borrow is initialized even before the values are compared, leading to a
            // noticeable slow down.
            panic!(r#"assertion failed: `(left != right)`
  left: `{:?}`,
 right: `{:?}`"#, &*left_val, &*right_val)
          } else {
            // no printing :(
            #[allow(unused_unsafe)]
            unsafe { $crate::intrinsics::__geobacter_kill() };
          }
        }
      }
    }
  });
  ($left:expr, $right:expr,) => ({
    $crate::host_assert_ne!($left, $right)
  });
  ($left:expr, $right:expr, $($arg:tt)+) => ({
    match (&$left, &$right) {
      (left_val, right_val) => {
        if *left_val == *right_val {
          if $crate::platform::is_host() {
            // The reborrows below are intentional. Without them, the stack slot for the
            // borrow is initialized even before the values are compared, leading to a
            // noticeable slow down.
            panic!(r#"assertion failed: `(left != right)`
  left: `{:?}`,
 right: `{:?}`"#, &*left_val, &*right_val,
                   format_args!($($arg)+));
          } else {
            // no printing :(
            #[allow(unused_unsafe)]
            unsafe { $crate::intrinsics::__geobacter_kill() };
          }
        }
      }
    }
  });
}

#[macro_export]
macro_rules! host_debug_assert {
    ($($arg:tt)*) => (if cfg!(debug_assertions) { $crate::host_assert!($($arg)*); })
}
#[macro_export]
macro_rules! host_debug_assert_eq {
    ($($arg:tt)*) => (if cfg!(debug_assertions) { $crate::host_assert_eq!($($arg)*); })
}
#[macro_export]
macro_rules! host_debug_assert_ne {
    ($($arg:tt)*) => (if cfg!(debug_assertions) { $crate::host_assert_ne!($($arg)*); })
}