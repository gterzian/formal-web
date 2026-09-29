// WebRTC loopback: two RTCPeerConnections in one formal-web page exchange text
// and binary messages over a data channel, through the WebRTC process.
//
//   xvfb-run -a node tests/webrtc/loopback.mjs [path/to/formal-web]
//
// Starts formal-web as a WebDriver server, loads a blank page from a local
// HTTP server, runs the scenario in the page and polls for its result.
// Exits non-zero when a check fails.
import http from 'node:http';
import net from 'node:net';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const binary = process.argv[2] || path.join(root, 'target/debug/formal-web');

const freePort = () => new Promise((resolve) => {
  const server = net.createServer().listen(0, '127.0.0.1', () => {
    const { port } = server.address();
    server.close(() => resolve(port));
  });
});

const port = await freePort();
const browser = spawn(binary, ['webdriver', '--headless', '--port', String(port), '--exit-on-session-delete'], {
  stdio: ['ignore', 'inherit', 'inherit'],
  // Its own process group, so the helper processes it starts end with it.
  detached: true,
});

const request = (method, p, body) => new Promise((resolve, reject) => {
  const data = body ? JSON.stringify(body) : '';
  const r = http.request({
    host: '127.0.0.1', port, path: p, method,
    headers: { 'content-type': 'application/json', 'content-length': Buffer.byteLength(data) },
  }, (response) => {
    let text = '';
    response.on('data', (chunk) => { text += chunk; });
    response.on('end', () => { try { resolve(JSON.parse(text)); } catch { resolve(text); } });
  });
  r.on('error', reject);
  if (data) r.write(data);
  r.end();
});

const page = http.createServer((q, r) => {
  r.setHeader('content-type', 'text/html');
  r.end('<!doctype html><title>webrtc loopback</title><p>loopback</p>');
}).listen(0, '127.0.0.1');
await new Promise((r) => page.on('listening', r));

// The scenario, run in the page. It records its progress in
// window.__webrtc so a failure says how far it got.
const scenario = `
window.__webrtc = { steps: [] };
const log = (s) => window.__webrtc.steps.push(s);
(async () => {
  const a = new RTCPeerConnection();
  const b = new RTCPeerConnection();
  const events = { a: [], b: [] };
  for (const [name, pc] of [['a', a], ['b', b]]) {
    pc.onsignalingstatechange = () => events[name].push('signaling:' + pc.signalingState);
    pc.onconnectionstatechange = () => events[name].push('connection:' + pc.connectionState);
    pc.onicegatheringstatechange = () => events[name].push('gathering:' + pc.iceGatheringState);
  }
  a.onicecandidate = (e) => { if (e.candidate && e.candidate.candidate) b.addIceCandidate(e.candidate).catch((err) => log('b.addIceCandidate: ' + err)); };
  b.onicecandidate = (e) => { if (e.candidate && e.candidate.candidate) a.addIceCandidate(e.candidate).catch((err) => log('a.addIceCandidate: ' + err)); };
  const negotiationNeeded = new Promise((r) => { a.onnegotiationneeded = () => r(true); });
  const dc = a.createDataChannel('chat', { protocol: 'test' });
  log('created ' + dc.label + ' ' + dc.readyState);
  await negotiationNeeded;
  log('negotiationneeded');
  const remote = new Promise((r) => { b.ondatachannel = (e) => r(e.channel); });
  const offer = await a.createOffer();
  log('offer ' + offer.type + ' ' + (offer.sdp.includes('m=application') ? 'm=application' : 'no data section'));
  await a.setLocalDescription(offer);
  log('a local ' + a.signalingState + ' ' + (a.localDescription && a.localDescription.type));
  await b.setRemoteDescription(offer);
  log('b remote ' + b.signalingState);
  await b.setLocalDescription();
  log('b answered ' + b.signalingState + ' ' + b.localDescription.type);
  await a.setRemoteDescription(b.localDescription);
  log('a stable ' + a.signalingState);
  const bc = await remote;
  log('datachannel ' + bc.label + ' ' + bc.protocol + ' ' + bc.readyState);
  if (dc.readyState !== 'open') await new Promise((r) => { dc.onopen = r; });
  log('open ' + dc.readyState + ' id=' + dc.id);
  bc.binaryType = 'arraybuffer';
  const received = [];
  const allReceived = new Promise((r) => { bc.onmessage = (e) => { received.push(e.data); if (received.length === 2) r(); }; });
  dc.send('hello ü 🦀');
  dc.send(new Uint8Array([1, 2, 3, 250]));
  await allReceived;
  const text = received[0];
  const bytes = Array.from(new Uint8Array(received[1]));
  log('received ' + JSON.stringify(text) + ' ' + JSON.stringify(bytes));
  const echo = new Promise((r) => { dc.onmessage = (e) => r(e.data); });
  bc.send('pong');
  const back = await echo;
  log('echo ' + back);
  const closed = new Promise((r) => { bc.onclose = () => r('closed'); });
  dc.close();
  log('closing ' + dc.readyState);
  log('remote ' + await closed);
  window.__webrtc.result = {
    ok: text === 'hello ü 🦀' && JSON.stringify(bytes) === '[1,2,3,250]' && back === 'pong',
    events,
    candidateType: typeof RTCIceCandidate,
  };
  a.close(); b.close();
  log('closed ' + a.signalingState + ' ' + a.connectionState);
})().catch((e) => { window.__webrtc.result = { ok: false, error: String(e && (e.name + ': ' + e.message) || e) }; });
return 'started';
`;

let exitCode = 1;
try {
  for (let i = 0; i < 120; i++) {
    try { await request('GET', '/status'); break; } catch { await new Promise((r) => setTimeout(r, 500)); }
  }
  const session = await request('POST', '/session', { capabilities: {} });
  const sid = session.value.sessionId;
  await request('POST', `/session/${sid}/url`, { url: `http://127.0.0.1:${page.address().port}/` });
  const started = await request('POST', `/session/${sid}/execute/sync`, { script: scenario, args: [] });
  console.log('start:', JSON.stringify(started.value));
  let state = null;
  for (let i = 0; i < 60; i++) {
    await new Promise((r) => setTimeout(r, 500));
    const polled = await request('POST', `/session/${sid}/execute/sync`, { script: 'return JSON.stringify(window.__webrtc || null)', args: [] });
    state = JSON.parse(polled.value);
    if (state && state.result && state.steps.at(-1)?.startsWith('closed')) break;
    if (state && state.result && !state.result.ok) break;
  }
  console.log(JSON.stringify(state, null, 2));
  exitCode = state && state.result && state.result.ok ? 0 : 1;
  await request('DELETE', `/session/${sid}`).catch(() => {});
} finally {
  page.close();
  try { process.kill(-browser.pid, "SIGTERM"); } catch { browser.kill(); }
}
process.exit(exitCode);
