use std::ffi::c_void;

use block2::RcBlock;
use napi::bindgen_prelude::*;
use napi::threadsafe_function::{ThreadsafeCallContext, ThreadsafeFunctionCallMode};
use napi_derive::napi;

#[napi(object, object_from_js = false)]
pub struct NWPath {
  pub status: NWPathStatus,
  pub is_expensive: bool,
  pub is_constrained: bool,
  pub has_ipv4: bool,
  pub has_ipv6: bool,
  pub has_dns: bool,
}

#[napi]
/// Interface types represent the underlying media for a network link, such as Wi-Fi or Cellular.
pub enum NWInterfaceType {
  /// nw_interface_type_other A virtual or otherwise unknown interface type
  Other,
  /// nw_interface_type_wifi A Wi-Fi link
  Wifi,
  /// nw_interface_type_wifi A Cellular link
  Cellular,
  /// nw_interface_type_wired A Wired Ethernet link
  Wired,
  /// nw_interface_type_loopback A Loopback link
  Loopback,
}

#[napi(string_enum)]
/// A network path status indicates if there is a usable route available upon which to send and receive data.
pub enum NWPathStatus {
  /// nw_path_status_invalid The path is not valid
  Invalid,
  /// nw_path_status_satisfied The path is valid and satisfies the required constraints
  Satisfied,
  /// nw_path_status_unsatisfied The path is valid, but does not satisfy the required constraints
  Unsatisfied,
  /// nw_path_status_satisfiable The path is potentially valid, but a connection is required
  Satisfiable,
  /// Reserved for future use
  Unknown,
}

impl From<ffi::nw_path_status_t> for NWPathStatus {
  fn from(status: ffi::nw_path_status_t) -> Self {
    match status {
      ffi::nw_path_status_t::NW_PATH_STATUS_INVALID => NWPathStatus::Invalid,
      ffi::nw_path_status_t::NW_PATH_STATUS_SATISFIED => NWPathStatus::Satisfied,
      ffi::nw_path_status_t::NW_PATH_STATUS_UNSATISFIED => NWPathStatus::Unsatisfied,
      ffi::nw_path_status_t::NW_PATH_STATUS_SATISFIABLE => NWPathStatus::Satisfiable,
      _ => NWPathStatus::Unknown,
    }
  }
}

impl From<NWInterfaceType> for ffi::nw_interface_type_t {
  fn from(interface_type: NWInterfaceType) -> Self {
    match interface_type {
      NWInterfaceType::Other => 0,
      NWInterfaceType::Wifi => 1,
      NWInterfaceType::Cellular => 2,
      NWInterfaceType::Wired => 3,
      NWInterfaceType::Loopback => 4,
    }
  }
}

#[napi]
/// A monitor that watches for changes in network path status.
pub struct NWPathMonitor {
  pm: ffi::nw_path_monitor_t,
}

#[napi]
impl NWPathMonitor {
  #[napi(constructor)]
  #[allow(clippy::new_without_default)]
  pub fn new() -> Self {
    let monitor = unsafe { ffi::nw_path_monitor_create() };
    let queue =
      unsafe { ffi::dispatch_get_global_queue(ffi::dispatch_qos_class_t::QOS_CLASS_DEFAULT, 0) };
    unsafe { ffi::nw_path_monitor_set_queue(monitor, queue.cast()) };
    Self { pm: monitor }
  }

  #[napi(factory)]
  /// Create a new path monitor with the specified interface type.
  pub fn new_with_type(interface_type: NWInterfaceType) -> Self {
    let monitor = unsafe { ffi::nw_path_monitor_create_with_type(interface_type.into()) };
    let queue =
      unsafe { ffi::dispatch_get_global_queue(ffi::dispatch_qos_class_t::QOS_CLASS_DEFAULT, 0) };
    unsafe { ffi::nw_path_monitor_set_queue(monitor, queue.cast()) };
    Self { pm: monitor }
  }

  #[napi]
  /// Start the path monitor, it will keep the Node.js alive unless you call stop on it.
  pub fn start(&mut self, on_update: Function<NWPath, ()>) -> Result<()> {
    let change_handler = on_update
      .build_threadsafe_function()
      .callee_handled::<false>()
      .weak::<false>()
      .build_callback(ctx_to_path)?;
    let cb = move |path: *mut c_void| {
      change_handler.call(path.cast(), ThreadsafeFunctionCallMode::NonBlocking);
    };
    unsafe {
      ffi::nw_path_monitor_set_update_handler(self.pm, &RcBlock::new(cb));
    };
    unsafe { ffi::nw_path_monitor_start(self.pm) };
    Ok(())
  }

  #[napi]
  /// Start the path monitor with weak reference, it will not keep the Node.js alive.
  pub fn start_weak(&mut self, on_update: Function<NWPath, ()>) -> Result<()> {
    let change_handler = on_update
      .build_threadsafe_function()
      .callee_handled::<false>()
      .weak::<true>()
      .build_callback(ctx_to_path)?;
    let cb = move |path: *mut c_void| {
      change_handler.call(path.cast(), ThreadsafeFunctionCallMode::NonBlocking);
    };
    unsafe {
      ffi::nw_path_monitor_set_update_handler(self.pm, &RcBlock::new(cb));
    };
    unsafe { ffi::nw_path_monitor_start(self.pm) };
    Ok(())
  }

  #[napi]
  /// Stop the path monitor.
  ///
  /// If you don't call this method and leave the monitor alone, it will be stopped automatically when it is GC.
  pub fn stop(&mut self) -> Result<()> {
    unsafe { ffi::nw_path_monitor_cancel(self.pm) };
    Ok(())
  }
}

#[inline]
fn ctx_to_path(ctx: ThreadsafeCallContext<ffi::nw_path_t>) -> Result<NWPath> {
  Ok(NWPath {
    status: unsafe { ffi::nw_path_get_status(ctx.value).into() },
    is_expensive: unsafe { ffi::nw_path_is_expensive(ctx.value) },
    is_constrained: unsafe { ffi::nw_path_is_constrained(ctx.value) },
    has_ipv4: unsafe { ffi::nw_path_has_ipv4(ctx.value) },
    has_ipv6: unsafe { ffi::nw_path_has_ipv6(ctx.value) },
    has_dns: unsafe { ffi::nw_path_has_dns(ctx.value) },
  })
}

#[allow(non_camel_case_types)]
#[allow(unused)]
mod ffi {
  use core::ffi::{c_int, c_uint, c_void};

  use block2::Block;

  macro_rules! enum_with_val {
    ($(#[$meta:meta])* $vis:vis struct $ident:ident($innervis:vis $ty:ty) {
      $($(#[$varmeta:meta])* $variant:ident = $num:expr),* $(,)*
    }) => {
      $(#[$meta])*
      #[repr(transparent)]
      $vis struct $ident($innervis $ty);
      impl $ident {
        $($(#[$varmeta])* $vis const $variant: $ident = $ident($num);)*
      }

      impl ::core::fmt::Debug for $ident {
        fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
          match self {
            $(&$ident::$variant => write!(f, "{}::{}", stringify!($ident), stringify!($variant)),)*
              &$ident(v) => write!(f, "UNKNOWN({})", v),
          }
        }
      }
    }
  }

  enum_with_val! {
    /// Quality-of-service classes that specify the priorities for executing tasks.
    #[derive(PartialEq, Eq, Clone, Copy)]
    pub struct dispatch_qos_class_t(pub c_uint) {
      QOS_CLASS_USER_INTERACTIVE = 0x21,
      QOS_CLASS_USER_INITIATED = 0x19,
      QOS_CLASS_DEFAULT = 0x15,
      QOS_CLASS_UTILITY = 0x11,
      QOS_CLASS_BACKGROUND = 0x09,
      QOS_CLASS_UNSPECIFIED = 0x00,
    }
  }

  enum_with_val! {
    /// A network path status indicates if there is a usable route available upon which to send and receive data.
    #[derive(PartialEq, Eq, Clone, Copy)]
    pub struct nw_path_status_t(pub c_uint) {
      NW_PATH_STATUS_INVALID = 0,
      NW_PATH_STATUS_SATISFIED = 1,
      NW_PATH_STATUS_UNSATISFIED = 2,
      NW_PATH_STATUS_SATISFIABLE = 3,
    }
  }

  #[repr(C)]
  // Dispatch.Framework
  // https://developer.apple.com/documentation/dispatch/dispatch_queue_t
  pub struct dispatch_queue {
    _unused: [u8; 0],
  }
  pub type dispatch_queue_t = *mut dispatch_queue;

  #[repr(C)]
  pub struct dispatch_queue_global {
    _unused: [u8; 0],
  }

  pub type dispatch_queue_global_t = *mut dispatch_queue_global;

  #[repr(C)]
  pub struct nw_interface {
    _unused: [u8; 0],
  }

  pub type nw_interface_t = *mut nw_interface;

  pub type nw_interface_type_t = c_int;

  #[repr(C)]
  pub struct nw_path {
    _unused: [u8; 0],
  }
  pub type nw_path_t = *mut nw_path;

  #[repr(C)]
  pub struct nw_path_monitor {
    _unused: [u8; 0],
  }
  pub type nw_path_monitor_t = *mut nw_path_monitor;

  #[cfg_attr(
    any(
      target_os = "macos",
      target_os = "ios",
      target_os = "tvos",
      target_os = "watchos",
      target_os = "visionos"
    ),
    link(name = "System", kind = "dylib")
  )]
  extern "C" {
    pub static _dispatch_main_q: dispatch_queue;
    /// Returns a system-defined global concurrent queue with the specified quality-of-service class.
    pub fn dispatch_get_global_queue(
      identifier: dispatch_qos_class_t,
      flags: usize,
    ) -> dispatch_queue_global_t;
  }
  #[cfg_attr(
    any(
      target_os = "macos",
      target_os = "ios",
      target_os = "tvos",
      target_os = "watchos",
      target_os = "visionos"
    ),
    link(name = "Network", kind = "framework")
  )]
  extern "C" {
    pub fn nw_path_monitor_create() -> nw_path_monitor_t;
    pub fn nw_path_monitor_create_with_type(
      required_interface_type: nw_interface_type_t,
    ) -> nw_path_monitor_t;

    // pub fn nw_path_monitor_set_cancel_handler(
    //   monitor: nw_path_monitor_t,
    //   cancel_handler: nw_path_monitor_cancel_handler_t,
    // );

    pub fn nw_path_monitor_set_update_handler(
      monitor: nw_path_monitor_t,
      update_handler: &Block<dyn Fn(*mut c_void)>,
    );
    pub fn nw_path_monitor_set_queue(monitor: nw_path_monitor_t, queue: dispatch_queue_t);
    pub fn nw_path_monitor_start(monitor: nw_path_monitor_t);
    pub fn nw_path_monitor_cancel(monitor: nw_path_monitor_t);
    pub fn nw_path_monitor_copy_current_path(monitor: nw_path_monitor_t) -> nw_path_t;

    pub fn nw_release(obj: *mut c_void);

    pub fn nw_path_get_status(path: nw_path_t) -> nw_path_status_t;
    pub fn nw_path_is_expensive(path: nw_path_t) -> bool;
    pub fn nw_path_is_constrained(path: nw_path_t) -> bool;
    pub fn nw_path_has_ipv4(path: nw_path_t) -> bool;
    pub fn nw_path_has_ipv6(path: nw_path_t) -> bool;
    pub fn nw_path_has_dns(path: nw_path_t) -> bool;
  }
}
