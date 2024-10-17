import { InternetMonitor } from './index.js'

const pm = new InternetMonitor()

console.log(`Network status: `, pm.current())

pm.start((path) => {
  console.log(`Network status: `, path)
  pm.stop();
  pm.start((path) => {
    console.log(`Network status inner: `, path)
  })
})

setTimeout(() => {
  // ref the pm, so that it doesn't get GCed
  pm.stop()
}, 1000000)
