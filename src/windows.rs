use std::borrow::Cow;
use std::mem::MaybeUninit;
use std::rc::Rc;
use std::sync::{
  atomic::{AtomicBool, AtomicU8, Ordering},
  Arc,
};

use bitflags::bitflags;
use napi::bindgen_prelude::*;
use napi::threadsafe_function::{
  ThreadsafeCallContext, ThreadsafeFunction, ThreadsafeFunctionCallMode,
};
use napi_derive::napi;
use windows::Win32::Foundation::{self, ERROR_BUFFER_OVERFLOW};
use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
use windows::Win32::Networking::NetworkListManager::*;
use windows::Win32::System::{self, Com::*};
use windows_core::{implement, IUnknown, Interface, HRESULT};

use crate::{NetworkInfo, NetworkStatus};

#[napi]
pub struct InternetMonitor {
  network_events_manager: INetworkEvents,
  cost_event_manager: INetworkCostManagerEvents,
  advise_network_list_manager_cookie: u32,
  advise_cost_manager_cookie: u32,
  network_list_manager: Rc<INetworkListManager>,
  network_list_manager_events_connection_point: IConnectionPoint,
  network_cost_manager: Rc<INetworkCostManager>,
  network_cost_manager_events_connection_point: IConnectionPoint,
  is_expensive: Arc<AtomicBool>,
  is_low_data_mode: Arc<AtomicBool>,
  has_ipv4: Arc<AtomicBool>,
  has_ipv6: Arc<AtomicBool>,
  has_dns: Arc<AtomicBool>,
  status: Arc<AtomicU8>,
}

#[napi::module_init]
fn init() {
  unsafe {
    // https://stackoverflow.com/a/2979671
    CoInitializeEx(None, COINIT_MULTITHREADED)
      .ok()
      .expect("CoInitializeEx failed");
  }
}

#[napi]
impl InternetMonitor {
  #[napi(constructor)]
  pub fn new() -> Result<Self> {
    // SAFETY: Windows API requires unsafe block
    unsafe {
      let network_list_manager: Rc<INetworkListManager> = Rc::new(
        CoCreateInstance(&NetworkListManager, None, CLSCTX_ALL).map_err(|_| {
          Error::new(
            Status::GenericFailure,
            "CoCreateInstance::CoCreateInstance INetworkListManager failed",
          )
        })?,
      );

      let network_cost_manager: Rc<INetworkCostManager> = Rc::new(
        CoCreateInstance(&NetworkListManager, None, CLSCTX_ALL).map_err(|_| {
          Error::new(
            Status::GenericFailure,
            "CoCreateInstance::CoCreateInstance INetworkCostManager failed",
          )
        })?,
      );

      let mut network_list_manager_connection_point_container: MaybeUninit<
        IConnectionPointContainer,
      > = MaybeUninit::uninit();
      network_list_manager
        .query(
          &IConnectionPointContainer::IID,
          network_list_manager_connection_point_container
            .as_mut_ptr()
            .cast(),
        )
        .ok()
        .map_err(|_| {
          Error::new(
            Status::GenericFailure,
            "INetworkListManager::QueryInterface failed",
          )
        })?;

      let mut network_cost_manager_connection_point_container: MaybeUninit<
        IConnectionPointContainer,
      > = MaybeUninit::uninit();
      network_cost_manager
        .query(
          &IConnectionPointContainer::IID,
          network_cost_manager_connection_point_container
            .as_mut_ptr()
            .cast(),
        )
        .ok()
        .map_err(|_| {
          Error::new(
            Status::GenericFailure,
            "INetworkCostManager::QueryInterface failed",
          )
        })?;

      // SAFETY: network_list_manager_connection_point_container is initialized when query is successful
      let network_list_manager_connection_point_container =
        network_list_manager_connection_point_container.assume_init();

      let network_list_manager_events_connection_point =
        network_list_manager_connection_point_container
          .FindConnectionPoint(&INetworkEvents::IID)
          .map_err(|_| {
            Error::new(
              Status::GenericFailure,
              "FindConnectionPoint::FindConnectionPoint(INetworkListManagerEvents) failed",
            )
          })?;

      // SAFETY: network_cost_manager_connection_point_container is initialized when query is successful
      let network_cost_manager_connection_point_container =
        network_cost_manager_connection_point_container.assume_init();

      let network_cost_manager_events_connection_point =
        network_cost_manager_connection_point_container
          .FindConnectionPoint(&INetworkCostManagerEvents::IID)
          .map_err(|_| {
            Error::new(
              Status::GenericFailure,
              "FindConnectionPoint::FindConnectionPoint(INetworkCostManagerEvents) failed",
            )
          })?;

      let mut network_info = NetworkInfo {
        has_ipv4: false,
        has_ipv6: false,
        has_dns: false,
        is_low_data_mode: false,
        is_expensive: false,
        status: NetworkStatus::Invalid,
      };

      let is_expensive = Arc::new(AtomicBool::new(false));
      let is_low_data_mode = Arc::new(AtomicBool::new(network_info.is_low_data_mode));
      let status = Arc::new(AtomicU8::new(network_info.status as u8));
      let mut get_network_info = || {
        {
          let connectivity = network_list_manager.GetConnectivity()?;

          let connections = network_list_manager.GetNetworkConnections()?;
          let mut all_connections = [None];
          connections.Next(&mut all_connections, None)?;
          if let Some(Some(connection)) = all_connections.first() {
            let mut network_connection_cost: MaybeUninit<INetworkConnectionCost> =
              MaybeUninit::uninit();
            connection
              .query(
                &INetworkConnectionCost::IID,
                network_connection_cost.as_mut_ptr().cast(),
              )
              .ok()?;
            let network_connection_cost = network_connection_cost.assume_init();
            let cost = network_connection_cost.GetCost()?;
            let mut data_plan = NLM_DATAPLAN_STATUS::default();
            network_connection_cost.GetDataPlanStatus(&mut data_plan)?;
            is_expensive.store(data_plan.DataLimitInMegabytes != u32::MAX, Ordering::SeqCst);
            is_low_data_mode.store(
              cost > NlmConnectionCost::UNRESTRICTED.bits(),
              Ordering::SeqCst,
            );
            network_info = get_network_info(
              connectivity,
              &is_expensive,
              &is_low_data_mode,
              &status,
              &network_list_manager,
            )?;
          }
        }
        Ok::<(), windows_core::Error>(())
      };

      get_network_info().map_err(|err| Error::new(Status::GenericFailure, format!("{err}")))?;

      let has_ipv4 = Arc::new(AtomicBool::new(network_info.has_ipv4));
      let has_ipv6 = Arc::new(AtomicBool::new(network_info.has_ipv6));
      let has_dns = Arc::new(AtomicBool::new(network_info.has_dns));

      Ok(Self {
        network_events_manager: NetworkEventsHandler {
          inner: Box::new(move |_status| {}),
          network_list_manager: network_list_manager.clone(),
          is_expensive: is_expensive.clone(),
          is_low_data_mode: is_low_data_mode.clone(),
          status: status.clone(),
        }
        .into(),
        cost_event_manager: NetworkCostEventsHandler {
          inner: Box::new(move |_status| {}),
          network_cost_manager: network_cost_manager.clone(),
          is_expensive: is_expensive.clone(),
          is_low_data_mode: is_low_data_mode.clone(),
          has_ipv4: has_ipv4.clone(),
          has_ipv6: has_ipv6.clone(),
          has_dns: has_dns.clone(),
          status: status.clone(),
        }
        .into(),
        advise_network_list_manager_cookie: 0,
        advise_cost_manager_cookie: 0,
        network_list_manager,
        network_list_manager_events_connection_point,
        network_cost_manager,
        network_cost_manager_events_connection_point,
        is_expensive,
        is_low_data_mode,
        has_ipv4,
        has_ipv6,
        has_dns,
        status,
      })
    }
  }

  #[napi]
  pub fn current(&self) -> NetworkInfo {
    NetworkInfo {
      is_expensive: self.is_expensive.load(Ordering::SeqCst),
      is_low_data_mode: self.is_low_data_mode.load(Ordering::SeqCst),
      has_ipv4: self.has_ipv4.load(Ordering::SeqCst),
      has_ipv6: self.has_ipv6.load(Ordering::SeqCst),
      has_dns: self.has_dns.load(Ordering::SeqCst),
      status: match self.status.load(Ordering::SeqCst) {
        0 => NetworkStatus::Invalid,
        1 => NetworkStatus::Satisfied,
        2 => NetworkStatus::Unsatisfied,
        3 => NetworkStatus::Satisfiable,
        4 => NetworkStatus::Unknown,
        _ => NetworkStatus::Invalid,
      },
    }
  }

  #[napi]
  /// Start the path monitor, it will keep the Node.js alive unless you call stop on it.
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
  /// Start the path monitor with weak reference, it will not keep the Node.js alive.
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

    // SAFETY: Windows API requires unsafe block
    unsafe {
      let network_event: INetworkEvents = NetworkEventsHandler {
        inner: Box::new(move |status| {
          change_handler.call(status, ThreadsafeFunctionCallMode::NonBlocking);
        }),
        network_list_manager: self.network_list_manager.clone(),
        is_expensive: self.is_expensive.clone(),
        is_low_data_mode: self.is_low_data_mode.clone(),
        status: self.status.clone(),
      }
      .into();
      let cost_event: INetworkCostManagerEvents = NetworkCostEventsHandler {
        inner: Box::new(move |status| {
          change_handler_for_cost.call(status, ThreadsafeFunctionCallMode::NonBlocking);
        }),
        network_cost_manager: self.network_cost_manager.clone(),
        is_expensive: self.is_expensive.clone(),
        is_low_data_mode: self.is_low_data_mode.clone(),
        has_ipv4: self.has_ipv4.clone(),
        has_ipv6: self.has_ipv6.clone(),
        has_dns: self.has_dns.clone(),
        status: self.status.clone(),
      }
      .into();
      let mut cost_event_handler = MaybeUninit::<IUnknown>::uninit();
      cost_event
        .query(&IUnknown::IID, cost_event_handler.as_mut_ptr().cast())
        .ok()
        .map_err(|_| {
          Error::new(
            Status::GenericFailure,
            "Failed to query IUnknown::IID on INetworkConnectionCostEvents",
          )
        })?;
      let cost_event_handler = cost_event_handler.assume_init();
      let advise_network_list_manager_cookie = self
        .network_list_manager_events_connection_point
        .Advise(&network_event)
        .map_err(handle_advise_error)?;
      let advise_cost_manager_cookie = self
        .network_cost_manager_events_connection_point
        .Advise(&cost_event_handler)
        .map_err(handle_advise_error)?;
      self.network_events_manager = network_event;
      self.cost_event_manager = cost_event;
      self.advise_network_list_manager_cookie = advise_network_list_manager_cookie;
      self.advise_cost_manager_cookie = advise_cost_manager_cookie;
    }
    Ok(())
  }

  #[napi]
  /// Stop the path monitor.
  ///
  /// If you don't call this method and leave the monitor alone, it will be stopped automatically when it is GC.
  pub fn stop(&mut self) -> Result<()> {
    // SAFETY: Windows API requires unsafe block
    unsafe {
      if self.advise_network_list_manager_cookie != 0 {
        self
          .network_list_manager_events_connection_point
          .Unadvise(self.advise_network_list_manager_cookie)
          .map_err(|_| {
            Error::new(
              Status::GenericFailure,
              "IConnectionPoint::Unadvise INetworkListManagerEvents failed",
            )
          })?;
      }

      if self.advise_cost_manager_cookie != 0 {
        self
          .network_cost_manager_events_connection_point
          .Unadvise(self.advise_cost_manager_cookie)
          .map_err(|_| {
            Error::new(
              Status::GenericFailure,
              "IConnectionPoint::Unadvise INetworkListManagerEvents failed",
            )
          })?;
      }

      // unref the ThreadsafeFunction
      self.network_events_manager = NetworkEventsHandler {
        inner: Box::new(move |_status| {}),
        network_list_manager: self.network_list_manager.clone(),
        is_expensive: self.is_expensive.clone(),
        is_low_data_mode: self.is_low_data_mode.clone(),
        status: self.status.clone(),
      }
      .into();

      // unref the ThreadsafeFunction
      self.cost_event_manager = NetworkCostEventsHandler {
        inner: Box::new(move |_status| {}),
        network_cost_manager: self.network_cost_manager.clone(),
        is_expensive: self.is_expensive.clone(),
        is_low_data_mode: self.is_low_data_mode.clone(),
        has_ipv4: self.has_ipv4.clone(),
        has_ipv6: self.has_ipv6.clone(),
        has_dns: self.has_dns.clone(),
        status: self.status.clone(),
      }
      .into();
    }
    Ok(())
  }
}

#[inline]
fn ctx_to_path(ctx: ThreadsafeCallContext<NetworkInfo>) -> Result<NetworkInfo> {
  Ok(ctx.value)
}

fn handle_advise_error(err: windows_core::Error) -> Error {
  let message = match err.code() {
    Foundation::E_POINTER => Cow::Borrowed("The value in pUnkSink or pdwCookie is not valid. For example, either pointer may be NULL. "),
    System::Ole::CONNECT_E_ADVISELIMIT => {
      Cow::Borrowed("The connection point has already reached its limit of connections and cannot accept any more.")
    }
    System::Ole::CONNECT_E_CANNOTCONNECT => {
      Cow::Borrowed("The sink does not support the interface required by this connection point.")
    }
    _ => Cow::Owned(format!("{err}")),
  };
  Error::new(
    Status::GenericFailure,
    format!("IConnectionPoint::Advise INetworkConnectionCostEvents failed {message}",),
  )
}

#[implement(INetworkEvents)]
struct NetworkEventsHandler {
  inner: Box<dyn Fn(NetworkInfo)>,
  is_expensive: Arc<AtomicBool>,
  is_low_data_mode: Arc<AtomicBool>,
  status: Arc<AtomicU8>,
  network_list_manager: Rc<INetworkListManager>,
}

#[implement(INetworkCostManagerEvents)]
struct NetworkCostEventsHandler {
  inner: Box<dyn Fn(NetworkInfo)>,
  network_cost_manager: Rc<INetworkCostManager>,
  is_expensive: Arc<AtomicBool>,
  is_low_data_mode: Arc<AtomicBool>,
  has_ipv4: Arc<AtomicBool>,
  has_ipv6: Arc<AtomicBool>,
  has_dns: Arc<AtomicBool>,
  status: Arc<AtomicU8>,
}

impl INetworkEvents_Impl for NetworkEventsHandler_Impl {
  fn NetworkAdded(&self, _networkid: &windows_core::GUID) -> windows_core::Result<()> {
    Ok(())
  }

  fn NetworkDeleted(&self, _networkid: &windows_core::GUID) -> windows_core::Result<()> {
    Ok(())
  }

  fn NetworkConnectivityChanged(
    &self,
    _: &windows_core::GUID,
    new_connectivity: NLM_CONNECTIVITY,
  ) -> windows_core::Result<()> {
    (self.inner)(get_network_info(
      new_connectivity,
      &self.is_expensive,
      &self.is_low_data_mode,
      &self.status,
      &self.network_list_manager,
    )?);

    Ok(())
  }

  fn NetworkPropertyChanged(
    &self,
    _networkid: &windows_core::GUID,
    _flags: NLM_NETWORK_PROPERTY_CHANGE,
  ) -> windows_core::Result<()> {
    Ok(())
  }
}

bitflags! {
  #[derive(Debug)]
  pub struct NlmConnectionCost: u32 {
    const UNKNOWN = 0;
    const UNRESTRICTED = 0x1;
    const FIXED = 0x2;
    const VARIABLE = 0x4;
    const OVERDATALIMIT = 0x10000;
    const CONGESTED = 0x20000;
    const ROAMING = 0x40000;
    const APPROACHINGDATALIMIT = 0x80000;
  }
}

impl INetworkCostManagerEvents_Impl for NetworkCostEventsHandler_Impl {
  fn CostChanged(&self, newcost: u32, _pdestaddr: *const NLM_SOCKADDR) -> windows_core::Result<()> {
    let is_low_data_mode = newcost > NlmConnectionCost::UNRESTRICTED.bits();
    self
      .is_low_data_mode
      .store(is_low_data_mode, Ordering::SeqCst);
    (self.inner)(NetworkInfo {
      is_expensive: self.is_expensive.load(Ordering::SeqCst),
      is_low_data_mode,
      has_ipv4: self.has_ipv4.load(Ordering::SeqCst),
      has_ipv6: self.has_ipv6.load(Ordering::SeqCst),
      has_dns: self.has_dns.load(Ordering::SeqCst),
      status: match self.status.load(Ordering::SeqCst) {
        0 => NetworkStatus::Invalid,
        1 => NetworkStatus::Satisfied,
        2 => NetworkStatus::Unsatisfied,
        3 => NetworkStatus::Satisfiable,
        4 => NetworkStatus::Unknown,
        _ => return Err(windows_core::Error::empty()),
      },
    });
    Ok(())
  }

  fn DataPlanStatusChanged(&self, pdestaddr: *const NLM_SOCKADDR) -> windows_core::Result<()> {
    let mut data_plan_status = NLM_DATAPLAN_STATUS::default();
    unsafe {
      self
        .network_cost_manager
        .GetDataPlanStatus(&mut data_plan_status, pdestaddr)?
    };
    let is_unlimited = data_plan_status.DataLimitInMegabytes == u32::MAX;
    if is_unlimited {
      self.is_expensive.store(false, Ordering::SeqCst);
      self.is_low_data_mode.store(false, Ordering::SeqCst);
    }
    self.is_expensive.store(!is_unlimited, Ordering::SeqCst);
    (self.inner)(NetworkInfo {
      is_expensive: !is_unlimited,
      is_low_data_mode: self.is_low_data_mode.load(Ordering::SeqCst),
      has_ipv4: self.has_ipv4.load(Ordering::SeqCst),
      has_ipv6: self.has_ipv6.load(Ordering::SeqCst),
      has_dns: self.has_dns.load(Ordering::SeqCst),
      status: match self.status.load(Ordering::SeqCst) {
        0 => NetworkStatus::Invalid,
        1 => NetworkStatus::Satisfied,
        2 => NetworkStatus::Unsatisfied,
        3 => NetworkStatus::Satisfiable,
        4 => NetworkStatus::Unknown,
        _ => NetworkStatus::Invalid,
      },
    });
    Ok(())
  }
}

fn get_available_connections<
  F: FnMut(
    &windows::Win32::NetworkManagement::IpHelper::IP_ADAPTER_ADDRESSES_LH,
  ) -> windows_core::Result<bool>,
>(
  mut callback: F,
) -> windows_core::Result<()> {
  use windows::Win32::NetworkManagement::IpHelper::{
    GetAdaptersAddresses, GAA_FLAG_INCLUDE_ALL_INTERFACES, IP_ADAPTER_ADDRESSES_LH,
  };
  use windows::Win32::Networking::WinSock::AF_UNSPEC;

  unsafe {
    let mut buffer_length = 0;
    let code = GetAdaptersAddresses(
      AF_UNSPEC.0 as u32,
      GAA_FLAG_INCLUDE_ALL_INTERFACES,
      None,
      None,
      &mut buffer_length,
    );

    // https://github.com/microsoft/windows-rs/issues/2832#issuecomment-1922306953
    // ERROR_BUFFER_OVERFLOW is expected because the buffer length is initially 0
    if code != 0x00000000 && code != ERROR_BUFFER_OVERFLOW.0 {
      return HRESULT::from_win32(code).ok();
    }

    let mut buffer = vec![0u8; buffer_length as usize];
    let addresses = buffer.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH;
    let code = GetAdaptersAddresses(
      AF_UNSPEC.0 as u32,
      GAA_FLAG_INCLUDE_ALL_INTERFACES,
      None,
      Some(addresses),
      &mut buffer_length,
    );
    if code != 0x00000000 {
      return HRESULT::from_win32(code).ok();
    }
    let mut current_addresses = addresses;
    while !current_addresses.is_null() {
      let adapter = &*current_addresses;
      if !callback(adapter)? {
        return Ok(());
      }
      current_addresses = adapter.Next;
    }
    Ok(())
  }
}

fn has_available_connections() -> windows_core::Result<bool> {
  let mut available = false;
  get_available_connections(|adapter| {
    if adapter.OperStatus == IfOperStatusUp {
      // break the iterator
      available = true;
      Ok(false)
    } else {
      Ok(true)
    }
  })?;
  Ok(available)
}

fn has_dns() -> windows_core::Result<bool> {
  let mut has_dns = false;
  get_available_connections(|adapter| {
    if adapter.OperStatus == IfOperStatusUp {
      // break the iterator
      has_dns = !adapter.FirstDnsServerAddress.is_null();
      Ok(false)
    } else {
      Ok(true)
    }
  })?;
  Ok(has_dns)
}

fn get_network_info(
  connectivity: NLM_CONNECTIVITY,
  is_expensive: &Arc<AtomicBool>,
  is_low_data_mode: &Arc<AtomicBool>,
  network_status: &Arc<AtomicU8>,
  network_list_manager: &Rc<INetworkListManager>,
) -> windows_core::Result<NetworkInfo> {
  let ipv4_internet =
    connectivity.0 & NLM_CONNECTIVITY_IPV4_INTERNET.0 == NLM_CONNECTIVITY_IPV4_INTERNET.0;
  let ipv4_no_traffic =
    connectivity.0 & NLM_CONNECTIVITY_IPV4_NOTRAFFIC.0 == NLM_CONNECTIVITY_IPV4_NOTRAFFIC.0;
  let ipv6_internet =
    connectivity.0 & NLM_CONNECTIVITY_IPV6_INTERNET.0 == NLM_CONNECTIVITY_IPV6_INTERNET.0;
  let ipv6_no_traffic =
    connectivity.0 & NLM_CONNECTIVITY_IPV6_NOTRAFFIC.0 == NLM_CONNECTIVITY_IPV6_NOTRAFFIC.0;
  let is_connected_to_internet = unsafe { network_list_manager.IsConnectedToInternet()? };
  let is_connected = unsafe { network_list_manager.IsConnected()? };
  let status = if is_connected_to_internet == true {
    NetworkStatus::Satisfied
  } else if is_connected == true && (ipv4_no_traffic || ipv6_no_traffic) {
    NetworkStatus::Unsatisfied
  } else if has_available_connections()? {
    NetworkStatus::Satisfiable
  } else {
    NetworkStatus::Invalid
  };
  network_status.store(status as u8, Ordering::SeqCst);
  Ok(NetworkInfo {
    has_ipv4: ipv4_internet,
    has_ipv6: ipv6_internet,
    has_dns: has_dns()?,
    is_low_data_mode: is_low_data_mode.load(Ordering::SeqCst),
    is_expensive: is_expensive.load(Ordering::SeqCst),
    status,
  })
}
