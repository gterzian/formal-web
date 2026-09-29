// Run a testharness.js page from tests/formal/tests (a bare file name) or
// from vendor/wpt (a path such as webrtc/RTCIceCandidate-constructor.html) in
// formal-web over
// WebDriver, without `wpt serve`: a local HTTP server serves the page, WPT's
// testharness.js from vendor/wpt/resources, and a testharnessreport.js that
// records the results in the page for this script to poll.
//
//   xvfb-run -a node tests/webrtc/testharness.mjs <test.html> [path/to/formal-web]
//
// Prints each subtest and exits non-zero when one does not pass.
import http from 'node:http';
import net from 'node:net';
import { readFileSync } from 'node:fs';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const test = process.argv[2] || 'webrtc-data-channel-loopback.html';
const binary = process.argv[3] || path.join(root, 'target/debug/formal-web');

const REPORT = `
add_completion_callback((tests, status) => {
  window.__testharness = {
    status: status.status,
    message: status.message,
    tests: tests.map((t) => ({ name: t.name, status: t.status, message: t.message })),
  };
});`;

const page = http.createServer((request, response) => {
  const url = new URL(request.url, 'http://localhost');
  try {
    if (url.pathname === '/resources/testharnessreport.js') {
      response.setHeader('content-type', 'text/javascript');
      return response.end(REPORT);
    }
    if (url.pathname.startsWith('/resources/')) {
      response.setHeader('content-type', 'text/javascript');
      return response.end(readFileSync(path.join(root, 'vendor/wpt', url.pathname)));
    }
    const file = url.pathname.slice(1).includes('/')
      ? path.join(root, 'vendor/wpt', path.normalize(url.pathname))
      : path.join(root, 'tests/formal/tests', path.basename(url.pathname));
    response.setHeader('content-type', file.endsWith('.js') ? 'text/javascript' : 'text/html');
    return response.end(readFileSync(file));
  } catch {
    response.statusCode = 404;
    return response.end();
  }
}).listen(0, '127.0.0.1');
await new Promise((r) => page.on('listening', r));

const port = await new Promise((resolve) => {
  const server = net.createServer().listen(0, '127.0.0.1', () => {
    const { port } = server.address();
    server.close(() => resolve(port));
  });
});
const browser = spawn(binary, ['webdriver', '--headless', '--port', String(port), '--exit-on-session-delete'], {
  stdio: ['ignore', 'ignore', 'inherit'],
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

// testharness.js status codes.
const TEST_STATUS = ['PASS', 'FAIL', 'TIMEOUT', 'NOTRUN', 'PRECONDITION_FAILED'];
const HARNESS_STATUS = ['OK', 'ERROR', 'TIMEOUT', 'PRECONDITION_FAILED'];

let exitCode = 1;
try {
  for (let i = 0; i < 120; i++) {
    try { await request('GET', '/status'); break; } catch { await new Promise((r) => setTimeout(r, 500)); }
  }
  const session = await request('POST', '/session', { capabilities: {} });
  const sid = session.value.sessionId;
  await request('POST', `/session/${sid}/url`, { url: `http://127.0.0.1:${page.address().port}/${test}` });
  let results = null;
  for (let i = 0; i < 60 && !results; i++) {
    await new Promise((r) => setTimeout(r, 500));
    const polled = await request('POST', `/session/${sid}/execute/sync`, {
      script: 'return JSON.stringify(window.__testharness || null)',
      args: [],
    });
    results = JSON.parse(polled.value);
  }
  if (!results) {
    console.log(`${test}: no result`);
  } else {
    console.log(`${test}: harness ${HARNESS_STATUS[results.status]}${results.message ? ` (${results.message})` : ''}`);
    for (const t of results.tests) {
      console.log(`  ${TEST_STATUS[t.status].padEnd(7)} ${t.name}${t.message ? `: ${t.message}` : ''}`);
    }
    exitCode = results.status === 0 && results.tests.every((t) => t.status === 0) ? 0 : 1;
  }
  await request('DELETE', `/session/${sid}`).catch(() => {});
} finally {
  page.close();
  try { process.kill(-browser.pid, "SIGTERM"); } catch { browser.kill(); }
}
process.exit(exitCode);
