import { eq } from 'lodash-es'

import { NwPathMonitor } from './index.js'

const pm = new NwPathMonitor()

/**
 * @type {import('./index.js').NwPath}
 */
const currentPath = {
  status: 'Unsatisfied',
  isExpensive: false,
  isConstrained: false,
  hasDns: false,
  hasIpv4: false,
  hasIpv6: false,
}

pm.start((path) => {
  if (!eq(path, currentPath)) {
    Object.assign(currentPath, path)
    console.log(`Network status: `, currentPath)
  }
})
