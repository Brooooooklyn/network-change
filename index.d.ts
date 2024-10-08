/* auto-generated by NAPI-RS */
/* eslint-disable */
/** A monitor that watches for changes in network path status. */
export declare class NwPathMonitor {
  constructor()
  /** Create a new path monitor with the specified interface type. */
  static newWithType(interfaceType: NWInterfaceType): NwPathMonitor
  /** Start the path monitor, it will keep the Node.js alive unless you call stop on it. */
  start(onUpdate: (arg: NwPath) => void): void
  /** Start the path monitor with weak reference, it will not keep the Node.js alive. */
  startWeak(onUpdate: (arg: NwPath) => void): void
  /**
   * Stop the path monitor.
   *
   * If you don't call this method and leave the monitor alone, it will be stopped automatically when it is GC.
   */
  stop(): void
}
export type NWPathMonitor = NwPathMonitor

/** Interface types represent the underlying media for a network link, such as Wi-Fi or Cellular. */
export declare enum NWInterfaceType {
  /** nw_interface_type_other A virtual or otherwise unknown interface type */
  Other = 0,
  /** nw_interface_type_wifi A Wi-Fi link */
  Wifi = 1,
  /** nw_interface_type_wifi A Cellular link */
  Cellular = 2,
  /** nw_interface_type_wired A Wired Ethernet link */
  Wired = 3,
  /** nw_interface_type_loopback A Loopback link */
  Loopback = 4
}

export interface NwPath {
  status: NWPathStatus
  isExpensive: boolean
  isConstrained: boolean
  hasIpv4: boolean
  hasIpv6: boolean
  hasDns: boolean
}

/** A network path status indicates if there is a usable route available upon which to send and receive data. */
export type NWPathStatus = /** nw_path_status_invalid The path is not valid */
'Invalid'|
/** nw_path_status_satisfied The path is valid and satisfies the required constraints */
'Satisfied'|
/** nw_path_status_unsatisfied The path is valid| but does not satisfy the required constraints */
'Unsatisfied'|
/** nw_path_status_satisfiable The path is potentially valid| but a connection is required */
'Satisfiable'|
/** Reserved for future use */
'Unknown';
