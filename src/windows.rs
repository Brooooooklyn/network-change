use std::mem::MaybeUninit;
use std::sync::atomic::AtomicBool;
use std::sync::{LazyLock, Mutex};

use windows::Win32::Networking::NetworkListManager::*;
use windows::Win32::System::Com::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows_core::{implement, Interface, HRESULT};

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
  mgr: NetworkListManagerEvents,
  advise_cookie: Option<u32>,
}

static global_handler: LazyLock<Mutex<Box<dyn Fn(ConnectionStatus) + Send + 'static>>> =
  LazyLock::new(|| Mutex::new(Box::new(|status: ConnectionStatus| {})));

static global_thread: LazyLock<Mutex<Option<std::thread::JoinHandle<()>>>> =
  LazyLock::new(|| Mutex::new(None));

static global_run_flag: LazyLock<AtomicBool> = LazyLock::new(|| AtomicBool::new(false));

#[napi]
impl InternetMonitor {
  #[napi(constructor)]
  pub fn new() -> Result<Self> {
    Ok(Self {
      mgr: NetworkListManagerEvents,
      advise_cookie: None,
    })
  }

  #[napi]
  pub fn start(&mut self, on_update: Function<ConnectionStatus, ()>) -> Result<()> {
    let change_handler = on_update
      .build_threadsafe_function()
      .callee_handled::<false>()
      .weak::<false>()
      .build_callback(ctx_to_path)?;

    *global_handler.lock().unwrap() = Box::new(move |status| {
      change_handler.call(status, ThreadsafeFunctionCallMode::NonBlocking);
    });

    // SAFETY: Windows API requires unsafe block
    unsafe {
      if CoInitialize(None).is_err() {
        return Err(Error::new(Status::GenericFailure, "CoInitialize failed"));
      }

      let network_list_manager: windows_core::Result<INetworkListManager> =
        CoCreateInstance(&NetworkListManager, None, CLSCTX_ALL);

      let mut connection_point_container: MaybeUninit<IConnectionPointContainer> =
        MaybeUninit::uninit();
      let mut hr: HRESULT = Default::default();
      if let Ok(network_list_manager) = network_list_manager {
        hr = network_list_manager.query(
          &IConnectionPointContainer::IID,
          connection_point_container.as_mut_ptr() as *mut _,
        );

        if hr.is_ok() {
          // SAFETY: connection_point_container is initialized when query is successful
          let connection_point_container: IConnectionPointContainer =
            connection_point_container.assume_init();

          let connection_point =
            connection_point_container.FindConnectionPoint(&INetworkListManagerEvents::IID);
          if hr.is_ok() {
            let connection_point: IConnectionPoint = connection_point.unwrap();

            let network_event: INetworkListManagerEvents = NetworkListManagerEvents.into();
            let advise_cookie_result = connection_point.Advise(&network_event);
            if advise_cookie_result.is_ok() {
              self.advise_cookie = Some(advise_cookie_result.unwrap());
            } else {
              return Err(Error::new(
                Status::GenericFailure,
                "IConnectionPoint::Advise failed",
              ));
            }

            global_run_flag.store(true, std::sync::atomic::Ordering::Release);

            let mut thread = global_thread.lock().unwrap();
            if thread.is_none() {
              thread.replace(std::thread::spawn(move || {
                while global_run_flag.load(std::sync::atomic::Ordering::Acquire) {
                  let mut msg: MSG = MSG::default();
                  while GetMessageA(&mut msg, None, 0, 0).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageA(&msg);
                  }
                }
              }));
            }
          } else {
            return Err(Error::new(
              Status::GenericFailure,
              "FindConnectionPoint::FindConnectionPoint failed",
            ));
          }
        } else {
          return Err(Error::new(
            Status::GenericFailure,
            "INetworkListManager::QueryInterface failed",
          ));
        }
      }
    }
    Ok(())
  }

  #[napi]
  pub fn stop(&mut self) -> Result<()> {
    global_run_flag.store(false, std::sync::atomic::Ordering::Release);
    global_thread
      .lock()
      .unwrap()
      .take()
      .and_then(|h| h.join().ok());
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

    global_handler.lock().unwrap()(ConnectionStatus {
      disconnected,
      ipv4_internet,
      ipv6_internet,
    });

    Ok(())
  }
}
