/* auto-generated by NAPI-RS */
/* eslint-disable */
export declare class InternetMonitor {
  constructor()
  current(): NetworkInfo
  /** Start the InternetMonitor, it will keep the Node.js alive unless you call stop on it. */
  start(onUpdate: (arg: NetworkInfo) => void): void
  /** Start the InternetMonitor with weak reference, it will not keep the Node.js alive. */
  startWeak(onUpdate: (arg: NetworkInfo) => void): void
  /**
   * Stop the InternetMonitor.
   *
   * If you don't call this method and leave the monitor alone, it will be stopped automatically when it is GC.
   */
  stop(): void
}

export interface NetworkInfo {
  status: NetworkStatus
  isExpensive: boolean
  isLowDataMode: boolean
  hasIpv4: boolean
  hasIpv6: boolean
  hasDns: boolean
}

/** A network path status indicates if there is a usable route available upon which to send and receive data. */
export type NetworkStatus = /** nw_path_status_invalid The path is not valid */
'Invalid'|
/** nw_path_status_satisfied The path is valid and satisfies the required constraints */
'Satisfied'|
/** nw_path_status_unsatisfied The path is valid| but does not satisfy the required constraints */
'Unsatisfied'|
/** nw_path_status_satisfiable The path is potentially valid| but a connection is required */
'Satisfiable'|
/** Reserved for future use */
'Unknown';
