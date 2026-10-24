// Minimal CDP Runtime.evaluate driver for the Android WebView.
// usage: node cdp.mjs '<js expression>'   (or pipe the expression on stdin)
const expr = process.argv[2] ?? (await new Promise((r) => {
  let s = ''
  process.stdin.on('data', (c) => (s += c))
  process.stdin.on('end', () => r(s))
}))

const targets = await (await fetch('http://127.0.0.1:9222/json')).json()
const page = targets.find((t) => t.type === 'page')
if (!page) throw new Error('no page target')

const ws = new WebSocket(page.webSocketDebuggerUrl)
await new Promise((res, rej) => {
  ws.addEventListener('open', res, { once: true })
  ws.addEventListener('error', rej, { once: true })
})

const result = await new Promise((res, rej) => {
  const id = 1
  const timer = setTimeout(() => rej(new Error('timeout')), 60000)
  ws.addEventListener('message', (event) => {
    const msg = JSON.parse(event.data)
    if (msg.id !== id) return
    clearTimeout(timer)
    res(msg)
  })
  ws.send(JSON.stringify({
    id,
    method: 'Runtime.evaluate',
    params: { expression: expr, awaitPromise: true, returnByValue: true },
  }))
})
ws.close()

if (result.result?.exceptionDetails) {
  console.error('EXCEPTION:', JSON.stringify(result.result.exceptionDetails.exception?.description ?? result.result.exceptionDetails))
  process.exit(1)
}
const value = result.result?.result?.value
console.log(typeof value === 'string' ? value : JSON.stringify(value, null, 2))
