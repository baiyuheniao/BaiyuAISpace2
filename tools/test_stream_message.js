#!/usr/bin/env node
// Local test harness: mock SSE LLM server + client to test SSE parsing, tool calls, and follow-up.
// Usage: node tools/test_stream_message.js

import http from 'http';
import { once } from 'events';
import { setTimeout as wait } from 'timers/promises';

const PORT = 41234;
const HOST = '127.0.0.1';

function readRequestBody(req) {
  return new Promise((resolve, reject) => {
    let body = '';
    req.on('data', (chunk) => (body += chunk));
    req.on('end', () => resolve(body));
    req.on('error', reject);
  });
}

async function startMockServer() {
  const server = http.createServer(async (req, res) => {
    if (req.method === 'POST' && req.url === '/stream') {
      // Simulate SSE stream responses
      res.writeHead(200, {
        'Content-Type': 'text/event-stream; charset=utf-8',
        'Cache-Control': 'no-cache',
        Connection: 'keep-alive',
      });

      // Helper to write an SSE `data:` message
      const writeData = async (obj, delay = 200) => {
        await wait(delay);
        res.write(`data: ${JSON.stringify(obj)}\n\n`);
      };

      // Send a few content deltas
      await writeData({ choices: [{ delta: { content: 'Hello' } }] });
      await writeData({ choices: [{ delta: { content: ' world' } }] });

      // Send a tool call delta (mimic OpenAI-like tool_calls in delta)
      await writeData({
        choices: [
          {
            delta: {
              tool_calls: [
                {
                  id: '1',
                  function: {
                    name: 'echo_tool',
                    arguments: JSON.stringify({ text: 'ping' }),
                  },
                },
              ],
            },
          },
        ],
      });

      // Done
      await writeData('[DONE]', 200);
      // Keep connection a moment then end
      await wait(50);
      res.end();
      return;
    }

    if (req.method === 'POST' && req.url === '/tools/call') {
      const body = await readRequestBody(req);
      let payload = {};
      try { payload = JSON.parse(body); } catch(e) {}
      // Simulate tool execution result
      const result = { success: true, result: { echoed: payload.arguments || payload } };
      res.writeHead(200, { 'Content-Type': 'application/json' });
      res.end(JSON.stringify(result));
      return;
    }

    if (req.method === 'POST' && req.url === '/once') {
      // Simulate non-stream LLM follow-up reply
      const body = await readRequestBody(req);
      let payload = {};
      try { payload = JSON.parse(body); } catch(e) {}
      const reply = { choices: [{ message: { content: 'Follow-up reply after tool result.' } }] };
      res.writeHead(200, { 'Content-Type': 'application/json' });
      res.end(JSON.stringify(reply));
      return;
    }

    res.writeHead(404);
    res.end('Not found');
  });

  server.listen(PORT, HOST);
  await once(server, 'listening');
  console.log(`Mock server listening at http://${HOST}:${PORT}`);
  return server;
}

function parseSseDataLine(data) {
  // data may be a JSON string or the literal "[DONE]"
  if (data === '[DONE]') return { type: 'done' };
  try {
    const json = JSON.parse(data);
    // Try OpenAI-style delta
    const delta = json.choices?.[0]?.delta;
    if (!delta) return null;
    if (typeof delta.content === 'string') return { type: 'text', text: delta.content };
    if (Array.isArray(delta.tool_calls)) return { type: 'tool_calls', calls: delta.tool_calls };
  } catch (e) {
    return null;
  }
  return null;
}

async function runClientTest() {
  const streamUrl = `http://${HOST}:${PORT}/stream`;
  console.log('Client: POST', streamUrl);
  const resp = await fetch(streamUrl, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ messages: [] }) });
  if (!resp.ok) throw new Error(`Stream request failed: ${resp.status}`);

  const reader = resp.body.getReader();
  const decoder = new TextDecoder();
  let buf = '';
  const accumulatedToolCalls = [];

  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    buf += decoder.decode(value, { stream: true });

    // Process complete SSE blocks separated by \n\n
    while (true) {
      const idx = buf.indexOf('\n\n');
      if (idx === -1) break;
      const block = buf.slice(0, idx);
      buf = buf.slice(idx + 2);

      // Each block may contain lines; process lines starting with 'data: '
      const lines = block.split(/\r?\n/);
      for (const line of lines) {
        if (!line.startsWith('data:')) continue;
        const data = line.slice(5).trim();
        const parsed = parseSseDataLine(data);
        if (!parsed) continue;
        if (parsed.type === 'text') {
          console.log('[chunk] text:', parsed.text);
        } else if (parsed.type === 'tool_calls') {
          console.log('[chunk] tool_calls detected:', parsed.calls);
          accumulatedToolCalls.push(...parsed.calls);
        } else if (parsed.type === 'done') {
          console.log('[chunk] DONE');
        }
      }
    }
  }

  // After stream finished, process accumulated tool calls
  if (accumulatedToolCalls.length > 0) {
    console.log('Processing', accumulatedToolCalls.length, 'tool calls...');
    for (const call of accumulatedToolCalls) {
      const func = call.function || call.function;
      const argsStr = func.arguments || func.arguments;
      let args = {};
      try { args = JSON.parse(argsStr); } catch (e) { args = { raw: argsStr }; }
      console.log(`Calling tool ${func.name} with`, args);
      const toolResp = await fetch(`http://${HOST}:${PORT}/tools/call`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ name: func.name, arguments: args }) });
      const toolResult = await toolResp.json();
      console.log('Tool result:', toolResult);

      // Send tool result back to LLM once for follow-up
      const followupResp = await fetch(`http://${HOST}:${PORT}/once`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ messages: [{ role: 'assistant', content: `工具 ${func.name} 调用结果：${JSON.stringify(toolResult)}` }] }) });
      const followUpJson = await followupResp.json();
      const followText = followUpJson.choices?.[0]?.message?.content || JSON.stringify(followUpJson);
      console.log('LLM follow-up reply:', followText);
    }
  }

  console.log('Client test finished.');
}

(async () => {
  const server = await startMockServer();
  try {
    await runClientTest();
  } catch (e) {
    console.error('Test failed:', e);
  } finally {
    server.close();
    console.log('Mock server closed.');
  }
})();
