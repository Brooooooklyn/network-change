use std::sync::{Arc, LazyLock, Mutex};

use napi::bindgen_prelude::*;
use napi::threadsafe_function::{
  ThreadsafeCallContext, ThreadsafeFunction, ThreadsafeFunctionCallMode,
};
use napi_derive::napi;

use glib::signal::SignalHandlerId;

use crate::NetworkInfo;
use crate::NetworkStatus;

const SIGNAL_NAME: &std::ffi::CStr = c"notify::connectivity";

static NETWORK_INFO: LazyLock<Mutex<NetworkInfo>> = LazyLock::new(|| {
  Mutex::new(NetworkInfo {
    status: NetworkStatus::Invalid,
    is_expensive: false,
    is_low_data_mode: false,
    has_ipv4: false,
    has_ipv6: false,
    has_dns: false,
  })
});

#[allow(clippy::type_complexity)]
static GLOBAL_HANDLER: LazyLock<Mutex<Option<Box<dyn Fn(NetworkInfo) + 'static + Send + Sync>>>> =
  LazyLock::new(|| Mutex::new(None));

#[derive(Copy, Clone)]
struct NMClientWrapper(*mut ffi::NMClient);
unsafe impl Send for NMClientWrapper {}
unsafe impl Sync for NMClientWrapper {}

#[napi]
pub struct InternetMonitor {
  client: NMClientWrapper,
  lo: Arc<Mutex<Option<glib::MainLoop>>>,
  signal_id: Arc<Mutex<Option<SignalHandlerId>>>,
  thread_handle: Option<std::thread::JoinHandle<()>>,
}

#[napi]
impl InternetMonitor {
  #[napi(constructor)]
  pub fn new() -> Result<Self> {
    let client = unsafe { ffi::nm_client_new(std::ptr::null_mut(), std::ptr::null_mut()) };
    if client.is_null() {
      return Err(Error::new(
        Status::GenericFailure,
        "Error initializing NetworkManager client.",
      ));
    }

    network_changed_cb(client);

    Ok(Self {
      client: NMClientWrapper(client),
      lo: Arc::new(Mutex::new(None)),
      signal_id: Arc::new(Mutex::new(None)),
      thread_handle: None,
    })
  }

  #[napi]
  pub fn current(&self) -> NetworkInfo {
    NETWORK_INFO.lock().unwrap().clone()
  }

  #[napi]
  /// Start the InternetMonitor, it will keep the Node.js alive unless you call stop on it.
  pub fn start(&mut self, on_update: Function<NetworkInfo, ()>) -> Result<()> {
    let change_handler = Arc::new(
      on_update
        .build_threadsafe_function()
        .callee_handled::<false>()
        .weak::<false>()
        .build_callback(ctx_to_path)?,
    );
    self.start_inner::<false>(change_handler)
  }

  #[napi]
  /// Start the InternetMonitor with weak reference, it will not keep the Node.js alive.
  pub fn start_weak(&mut self, on_update: Function<NetworkInfo, ()>) -> Result<()> {
    let change_handler = Arc::new(
      on_update
        .build_threadsafe_function()
        .callee_handled::<false>()
        .weak::<true>()
        .build_callback(ctx_to_path)?,
    );
    self.start_inner::<true>(change_handler)
  }

  fn start_inner<const WEAK: bool>(
    &mut self,
    change_handler: Arc<ThreadsafeFunction<NetworkInfo, (), NetworkInfo, false, { WEAK }>>,
  ) -> Result<()> {
    let change_handler_for_cost = change_handler.clone();

    GLOBAL_HANDLER
      .lock()
      .unwrap()
      .replace(Box::new(move |info| {
        change_handler_for_cost.call(info, ThreadsafeFunctionCallMode::Blocking);
      }));

    let lo = self.lo.clone();
    let client: NMClientWrapper = self.client;
    let signal_id = self.signal_id.clone();
    self.thread_handle = Some(std::thread::spawn(move || {
      let c = client;
      unsafe {
        signal_id
          .lock()
          .unwrap()
          .replace(glib::signal::connect_raw::<NetworkInfo>(
            (c.0) as *const _ as *mut glib::gobject_ffi::GObject,
            SIGNAL_NAME.as_ptr(),
            Some(std::mem::transmute::<*const (), unsafe extern "C" fn()>(
              network_changed_cb as *const (),
            )),
            std::ptr::null_mut(),
          ));
        lo.lock().unwrap().replace(glib::MainLoop::new(None, false));
        lo.lock().unwrap().as_ref().unwrap().run();
      }
    }));

    Ok(())
  }

  #[napi]
  /// Stop the InternetMonitor.
  ///
  /// If you don't call this method and leave the monitor alone, it will be stopped automatically when it is GC.
  pub fn stop(&mut self) {
    let signal_id = self.signal_id.lock().unwrap().take();
    if let Some(signal_id) = signal_id {
      unsafe {
        glib::signal::signal_handler_disconnect(
          &*self.client.0.cast::<glib::object::Object>(),
          signal_id,
        );
      }
    }

    let lo = self.lo.lock().unwrap().take();
    if let Some(lo) = lo {
      lo.quit();
    }

    self.thread_handle.take().unwrap().join().unwrap();
  }
}

#[inline]
fn ctx_to_path(ctx: ThreadsafeCallContext<NetworkInfo>) -> Result<NetworkInfo> {
  Ok(ctx.value)
}

extern "C" fn network_changed_cb(client: *mut ffi::NMClient) {
  let mut info = NETWORK_INFO.lock().unwrap();

  let metered = unsafe { ffi::nm_client_get_metered(client) };
  info.is_low_data_mode = matches!(
    metered,
    ffi::NMMetered::NM_METERED_YES | ffi::NMMetered::NM_METERED_GUESS_YES
  );

  let devices = unsafe { &*ffi::nm_client_get_devices(client) };
  for i in 0..devices.len {
    let device = unsafe { (devices.pdata as *mut *mut ffi::NMDevice).add(i as usize) };
    let device_type = unsafe { ffi::nm_device_get_device_type(*device) };

    // Check if the connection is expensive (e.g., mobile broadband)
    if device_type == ffi::NMDeviceType::NM_DEVICE_TYPE_MODEM {
      info.is_expensive = true;
    }

    // Check for IPv4 connectivity
    let ip4_config = unsafe { ffi::nm_device_get_ip4_config(*device) };
    if !ip4_config.is_null() {
      info.has_ipv4 = true;
    }

    // Check for IPv6 connectivity
    let ip6_config = unsafe { ffi::nm_device_get_ip6_config(*device) };
    if !ip6_config.is_null() {
      info.has_ipv6 = true;
    }
  }

  // Check DNS configuration from global NM settings
  let active_conn = unsafe { ffi::nm_client_get_primary_connection(client) };
  if !active_conn.is_null() {
    let ip_config = unsafe { ffi::nm_active_connection_get_ip4_config(active_conn) };
    if !ip_config.is_null() && !unsafe { ffi::nm_ip_config_get_nameservers(ip_config) }.is_null() {
      info.has_dns = true;
    }
  }

  // Determine network status
  let connectivity = unsafe { ffi::nm_client_get_connectivity(client) };
  match connectivity {
    ffi::NMConnectivityState::NM_CONNECTIVITY_FULL => {
      info.status = NetworkStatus::Satisfied;
    }
    ffi::NMConnectivityState::NM_CONNECTIVITY_LIMITED
    | ffi::NMConnectivityState::NM_CONNECTIVITY_PORTAL => {
      info.status = NetworkStatus::Satisfiable;
    }
    ffi::NMConnectivityState::NM_CONNECTIVITY_NONE => {
      info.status = NetworkStatus::Unsatisfied;
    }
    _ => {
      info.status = NetworkStatus::Invalid;
    }
  }

  if let Some(f) = GLOBAL_HANDLER.lock().unwrap().as_ref() {
    f(info.clone())
  }
}

#[allow(non_camel_case_types)]
#[allow(non_snake_case)]
#[allow(unused)]
mod ffi {
  use gio::Cancellable;
  use glib::error::Error as GError;
  pub use std::ffi::{c_int, c_void};

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
    #[derive(PartialEq, Eq, Clone, Copy)]
    pub struct NMConnectivityState(pub c_int) {
        NM_CONNECTIVITY_UNKNOWN = 0,
        NM_CONNECTIVITY_NONE    = 1,
        NM_CONNECTIVITY_PORTAL  = 2,
        NM_CONNECTIVITY_LIMITED = 3,
        NM_CONNECTIVITY_FULL    = 4,
    }
  }

  enum_with_val! {
    #[derive(PartialEq, Eq, Clone, Copy)]
    pub struct NMMetered(pub c_int) {
      NM_METERED_UNKNOWN   = 0,
      NM_METERED_YES       = 1,
      NM_METERED_NO        = 2,
      NM_METERED_GUESS_YES = 3,
      NM_METERED_GUESS_NO  = 4,
    }
  }

  #[repr(C)]
  pub struct NMClient {
    _unused: [u8; 0],
  }

  type gpointer = *mut c_void;
  type guint = u32;

  #[repr(C)]
  pub struct GPtrArray {
    pub pdata: *mut gpointer,
    pub len: guint,
  }

  #[repr(C)]
  pub struct NMDevice {
    _unused: [u8; 0],
  }

  #[repr(C)]
  pub struct NMActiveConnection {
    _unused: [u8; 0],
  }

  enum_with_val! {
      #[derive(PartialEq, Eq, Clone, Copy)]
      pub struct NMDeviceType(c_int) {
          NM_DEVICE_TYPE_UNKNOWN       = 0,
          NM_DEVICE_TYPE_ETHERNET      = 1,
          NM_DEVICE_TYPE_WIFI          = 2,
          NM_DEVICE_TYPE_UNUSED1       = 3,
          NM_DEVICE_TYPE_UNUSED2       = 4,
          NM_DEVICE_TYPE_BT            = 5, /* Bluetooth */
          NM_DEVICE_TYPE_OLPC_MESH     = 6,
          NM_DEVICE_TYPE_WIMAX         = 7,
          NM_DEVICE_TYPE_MODEM         = 8,
          NM_DEVICE_TYPE_INFINIBAND    = 9,
          NM_DEVICE_TYPE_BOND          = 10,
          NM_DEVICE_TYPE_VLAN          = 11,
          NM_DEVICE_TYPE_ADSL          = 12,
          NM_DEVICE_TYPE_BRIDGE        = 13,
          NM_DEVICE_TYPE_GENERIC       = 14,
          NM_DEVICE_TYPE_TEAM          = 15,
          NM_DEVICE_TYPE_TUN           = 16,
          NM_DEVICE_TYPE_IP_TUNNEL     = 17,
          NM_DEVICE_TYPE_MACVLAN       = 18,
          NM_DEVICE_TYPE_VXLAN         = 19,
          NM_DEVICE_TYPE_VETH          = 20,
          NM_DEVICE_TYPE_MACSEC        = 21,
          NM_DEVICE_TYPE_DUMMY         = 22,
          NM_DEVICE_TYPE_PPP           = 23,
          NM_DEVICE_TYPE_OVS_INTERFACE = 24,
          NM_DEVICE_TYPE_OVS_PORT      = 25,
          NM_DEVICE_TYPE_OVS_BRIDGE    = 26,
          NM_DEVICE_TYPE_WPAN          = 27,
          NM_DEVICE_TYPE_6LOWPAN       = 28,
          NM_DEVICE_TYPE_WIREGUARD     = 29,
          NM_DEVICE_TYPE_WIFI_P2P      = 30,
          NM_DEVICE_TYPE_VRF           = 31,
      }
  }

  #[repr(C)]
  pub struct NMIPConfig {
    _unused: [u8; 0],
  }

  #[cfg_attr(any(target_os = "linux",), link(name = "nm", kind = "dylib"))]
  extern "C" {
    pub fn nm_client_new(callcellable: *mut Cancellable, error: *mut GError) -> *mut NMClient;

    pub fn nm_client_get_devices(client: *mut NMClient) -> *mut GPtrArray;
    pub fn nm_device_get_device_type(device: *mut NMDevice) -> NMDeviceType;
    pub fn nm_device_get_ip4_config(device: *mut NMDevice) -> *mut NMIPConfig;
    pub fn nm_device_get_ip6_config(device: *mut NMDevice) -> *mut NMIPConfig;
    pub fn nm_client_get_primary_connection(device: *mut NMClient) -> *mut NMActiveConnection;
    pub fn nm_active_connection_get_ip4_config(device: *mut NMActiveConnection) -> *mut NMIPConfig;
    pub fn nm_ip_config_get_nameservers(ip_config: *mut NMIPConfig) -> *mut GPtrArray;
    pub fn nm_client_get_connectivity(client: *mut NMClient) -> NMConnectivityState;
    pub fn nm_client_get_metered(client: *mut NMClient) -> NMMetered;
  }
}
