#!/usr/bin/env node
// Driver for BaiyuAISpace2 (Tauri app). Drives the running app's WebView2
// via the Chrome DevTools Protocol (CDP) — no extra npm dependencies,
// just Node's built-in fetch/WebSocket/child_process.
//
// Usage:
//   node driver.mjs launch [path/to/BaiyuAISpace2.exe]
//   node driver.mjs screenshot <out.png>
//   node driver.mjs eval "<js expression>"
//   node driver.mjs nav <route>        e.g. "mcp", "local-deploy", "settings", "" (chat)
//   node driver.mjs click "<visible text>"
//   node driver.mjs quit
//
// State (CDP port, spawned PID) is kept in driver.state.json next to this
// file so each subcommand can be a short-lived process.

import { spawn } from "node:child_process";
import { writeFileSync, readFileSync, existsSync, unlinkSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const STATE_FILE = path.join(__dirname, "driver.state.json");
const CDP_PORT = Number(process.env.BAIYU_CDP_PORT || 9333);

function loadState() {
  if (!existsSync(STATE_FILE)) return {};
  try { return JSON.parse(readFileSync(STATE_FILE, "utf8")); } catch { return {}; }
}
function saveState(state) {
  writeFileSync(STATE_FILE, JSON.stringify(state, null, 2));
}

async function waitForPort(port, timeoutMs = 15000) {
  const start = Date.now();
  while (Date.now() - start < timeoutMs) {
    try {
      const res = await fetch(`http://127.0.0.1:${port}/json/version`);
      if (res.ok) return true;
    } catch { /* not up yet */ }
    await new Promise((r) => setTimeout(r, 300));
  }
  return false;
}

async function getPageTarget(port) {
  const list = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
  const page = list.find((t) => t.type === "page");
  if (!page) throw new Error("No page target found — is the app running with launch?");
  return page;
}

function cdpConnect(wsUrl) {
  const ws = new WebSocket(wsUrl);
  let id = 0;
  const pending = new Map();
  ws.onmessage = (ev) => {
    const msg = JSON.parse(ev.data);
    if (msg.id && pending.has(msg.id)) {
      const { resolve, reject } = pending.get(msg.id);
      pending.delete(msg.id);
      if (msg.error) reject(new Error(JSON.stringify(msg.error)));
      else resolve(msg.result);
    }
  };
  const send = (method, params = {}) =>
    new Promise((resolve, reject) => {
      const reqId = ++id;
      pending.set(reqId, { resolve, reject });
      ws.send(JSON.stringify({ id: reqId, method, params }));
    });
  const ready = new Promise((resolve) => (ws.onopen = resolve));
  return { ws, send, ready };
}

async function withPage(fn) {
  const page = await getPageTarget(CDP_PORT);
  const { ws, send, ready } = cdpConnect(page.webSocketDebuggerUrl);
  await ready;
  try {
    return await fn(send);
  } finally {
    ws.close();
  }
}

async function cmdLaunch(exePathArg) {
  const exePath = exePathArg || path.join(__dirname, "..", "..", "..", "src-tauri", "target", "debug", "BaiyuAISpace2.exe");
  if (!existsSync(exePath)) {
    console.error(`Executable not found: ${exePath}\nBuild it first: cd src-tauri && cargo build`);
    process.exit(1);
  }

  const state = loadState();
  if (state.pid) {
    console.error(`A driver-launched instance may already be running (PID ${state.pid}). Run "quit" first, or delete ${STATE_FILE} if it's stale.`);
    process.exit(1);
  }

  const child = spawn(exePath, [], {
    env: { ...process.env, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${CDP_PORT}` },
    detached: true,
    stdio: "ignore",
  });
  child.unref();

  const up = await waitForPort(CDP_PORT);
  if (!up) {
    console.error("App did not open the CDP port in time. Check that no other instance is holding the WebView2 user-data folder (close any other running BaiyuAISpace2.exe and retry).");
    process.exit(1);
  }

  saveState({ pid: child.pid, port: CDP_PORT });
  console.log(`Launched PID ${child.pid}, CDP ready on port ${CDP_PORT}`);
}

async function cmdQuit() {
  const state = loadState();
  if (!state.pid) {
    console.log("No driver-launched PID recorded — nothing to do.");
    return;
  }
  try {
    process.kill(state.pid);
    console.log(`Sent kill to PID ${state.pid}`);
  } catch (e) {
    console.log(`PID ${state.pid} was not running (${e.message})`);
  }
  unlinkSync(STATE_FILE);
}

async function cmdScreenshot(outFile) {
  if (!outFile) { console.error("usage: driver.mjs screenshot <out.png>"); process.exit(1); }
  await withPage(async (send) => {
    const shot = await send("Page.captureScreenshot", { format: "png" });
    writeFileSync(outFile, Buffer.from(shot.data, "base64"));
    console.log(`Saved ${outFile}`);
  });
}

async function cmdEval(expr) {
  if (!expr) { console.error("usage: driver.mjs eval \"<js expression>\""); process.exit(1); }
  await withPage(async (send) => {
    const result = await send("Runtime.evaluate", { expression: expr, returnByValue: true });
    if (result.exceptionDetails) {
      console.error("Eval threw:", result.exceptionDetails.text);
      process.exit(1);
    }
    console.log(result.result.value ?? result.result.description ?? "undefined");
  });
}

async function cmdNav(route) {
  const r = (route ?? "").replace(/^\/?#?\/?/, "");
  await withPage(async (send) => {
    await send("Runtime.evaluate", { expression: `window.location.hash = '#/${r}'` });
    console.log(`Navigated to #/${r}`);
  });
}

async function cmdClick(text) {
  if (!text) { console.error('usage: driver.mjs click "<visible text>"'); process.exit(1); }
  await withPage(async (send) => {
    const expr = `
      (() => {
        const candidates = Array.from(document.querySelectorAll('button, .n-menu-item-content, a, [role="button"]'));
        const target = candidates.find(el => el.textContent && el.textContent.includes(${JSON.stringify(text)}));
        if (!target) return 'NOT_FOUND';
        target.dispatchEvent(new MouseEvent('click', { bubbles: true }));
        return 'CLICKED: ' + target.textContent.trim();
      })()
    `;
    const result = await send("Runtime.evaluate", { expression: expr, returnByValue: true });
    console.log(result.result.value);
  });
}

const [, , cmd, ...args] = process.argv;

switch (cmd) {
  case "launch": await cmdLaunch(args[0]); break;
  case "quit": await cmdQuit(); break;
  case "screenshot": await cmdScreenshot(args[0]); break;
  case "eval": await cmdEval(args[0]); break;
  case "nav": await cmdNav(args[0]); break;
  case "click": await cmdClick(args[0]); break;
  default:
    console.error("Usage: node driver.mjs <launch|quit|screenshot|eval|nav|click> [args]");
    process.exit(1);
}
