use napi_derive::napi;

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "macos")]
pub use macos::*;

#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "windows")]
pub use windows::*;

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "linux")]
pub use linux::*;

#[napi(string_enum)]
#[repr(u8)]
#[derive(Debug, Clone, Copy)]
/// A network path status indicates if there is a usable route available upon which to send and receive data.
pub enum NetworkStatus {
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

#[napi(object, object_from_js = false)]
#[derive(Debug, Clone)]
pub struct NetworkInfo {
  pub status: NetworkStatus,
  pub is_expensive: bool,
  pub is_low_data_mode: bool,
  pub has_ipv4: bool,
  pub has_ipv6: bool,
  pub has_dns: bool,
}
