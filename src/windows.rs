use std::mem::MaybeUninit;
use std::sync::{LazyLock, Mutex};

use windows::Win32::Networking::NetworkListManager::*;
use windows::Win32::System::Com::*;
use windows_core::{implement, Interface};

use napi::bindgen_prelude::*;
use napi::threadsafe_function::{ThreadsafeCallContext, ThreadsafeFunctionCallMode};
use napi_derive::napi;

#[napi(object, object_from_js = false)]
pub struct ConnectionStatus {
  /// if WIFI:
  ///   true if the wifi is disconnected
  ///   false if the wifi is connected
  /// if ETHERNET:
  ///   true if the ethernet (cable) is disconnected
  ///   false if the ethernet (cable) is connected
  pub disconnected: bool,
  /// true if the ipv4 has internet
  /// false if the ipv4 has no internet
  pub ipv4_internet: bool,
  /// true if the ipv6 has internet
  /// false if the ipv6 has no internet
  pub ipv6_internet: bool,
}

#[napi]
pub struct InternetMonitor {
  mgr: Option<INetworkListManagerEvents>,
  advise_cookie: Option<u32>,
  network_list_manager: Option<INetworkListManager>,
  connection_point: Option<IConnectionPoint>,
  connection_point_container: Option<IConnectionPointContainer>,
}

static GLOBAL_HANDLER: LazyLock<Mutex<Option<Box<dyn Fn(ConnectionStatus) + Send + 'static>>>> =
  LazyLock::new(|| Mutex::new(None));

#[napi]
impl InternetMonitor {
  #[napi(constructor)]
  pub fn new() -> Result<Self> {
    Ok(Self {
      mgr: None,
      advise_cookie: None,
      network_list_manager: None,
      connection_point: None,
      connection_point_container: None,
    })
  }

  #[napi]
  pub fn start(&mut self, on_update: Function<ConnectionStatus, ()>) -> Result<()> {
    let change_handler = on_update
      .build_threadsafe_function()
      .callee_handled::<false>()
      .weak::<false>()
      .build_callback(ctx_to_path)?;

    *GLOBAL_HANDLER.lock().unwrap() = Some(Box::new(move |status| {
      change_handler.call(status, ThreadsafeFunctionCallMode::NonBlocking);
    }));

    // SAFETY: Windows API requires unsafe block
    unsafe {
      // https://stackoverflow.com/a/2979671
      CoInitializeEx(None, COINIT_MULTITHREADED)
        .ok()
        .map_err(|_| Error::new(Status::GenericFailure, "CoInitializeEx failed"))?;

      let network_list_manager: windows_core::Result<INetworkListManager> =
        CoCreateInstance(&NetworkListManager, None, CLSCTX_ALL);

      let mut connection_point_container: MaybeUninit<IConnectionPointContainer> =
        MaybeUninit::uninit();
      if let Ok(network_list_manager) = network_list_manager {
        network_list_manager
          .query(
            &IConnectionPointContainer::IID,
            connection_point_container.as_mut_ptr() as *mut _,
          )
          .ok()
          .map_err(|e| {
            Error::new(
              Status::GenericFailure,
              "INetworkListManager::QueryInterface failed",
            )
          })?;
        self.network_list_manager = Some(network_list_manager);

        // SAFETY: connection_point_container is initialized when query is successful
        let connection_point_container: IConnectionPointContainer =
          connection_point_container.assume_init();

        let connection_point = connection_point_container
          .FindConnectionPoint(&INetworkListManagerEvents::IID)
          .map_err(|_| {
            Error::new(
              Status::GenericFailure,
              "FindConnectionPoint::FindConnectionPoint failed",
            )
          })?;
        self.connection_point_container = Some(connection_point_container);

        let network_event: INetworkListManagerEvents = NetworkListManagerEvents.into();
        self.mgr = Some(network_event);
        let advise_cookie_result = connection_point
          .Advise(self.mgr.as_ref().unwrap())
          .map_err(|_| Error::new(Status::GenericFailure, "IConnectionPoint::Advise failed"));
        self.advise_cookie = Some(advise_cookie_result.unwrap());
        self.connection_point = Some(connection_point);
      }
    }
    Ok(())
  }

  #[napi]
  pub fn stop(&mut self) -> Result<()> {
    // SAFETY: Windows API requires unsafe block
    unsafe {
      let advise_cookie = self.advise_cookie.take();
      let connection_point = self.connection_point.take();

      // https://stackoverflow.com/a/33779802
      advise_cookie.and_then(|advise_cookie| {
        connection_point.map(|connection_point| connection_point.Unadvise(advise_cookie))
      });

      self.connection_point_container = None;
      self.mgr = None;
      *GLOBAL_HANDLER.lock().unwrap() = None;
      CoUninitialize();
    }
    Ok(())
  }
}

#[inline]
fn ctx_to_path(ctx: ThreadsafeCallContext<ConnectionStatus>) -> Result<ConnectionStatus> {
  Ok(ctx.value)
}

#[implement(INetworkListManagerEvents)]
struct NetworkListManagerEvents;
impl INetworkListManagerEvents_Impl for NetworkListManagerEvents_Impl {
  fn ConnectivityChanged(&self, new_connectivity: NLM_CONNECTIVITY) -> windows_core::Result<()> {
    let disconnected = new_connectivity.0 == 0;
    let ipv4_internet =
      new_connectivity.0 & NLM_CONNECTIVITY_IPV4_INTERNET.0 == NLM_CONNECTIVITY_IPV4_INTERNET.0;
    let ipv6_internet =
      new_connectivity.0 & NLM_CONNECTIVITY_IPV6_INTERNET.0 == NLM_CONNECTIVITY_IPV6_INTERNET.0;

    (*GLOBAL_HANDLER.lock().unwrap()).as_ref().unwrap()(ConnectionStatus {
      disconnected,
      ipv4_internet,
      ipv6_internet,
    });

    Ok(())
  }
}
