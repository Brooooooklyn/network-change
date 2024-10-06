import test from 'ava'

import { NwPathMonitor } from '../index.js'

test('should not throw while listening', (t) => {
  t.notThrows(() => {
    const pm = new NwPathMonitor()
    pm.start((path) => {
      console.info(path)
      pm.stop()
    })
  })
})
