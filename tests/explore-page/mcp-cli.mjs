// Drives `e2e mcp` from a shell, for an agent that has no e2e MCP server registered: it starts
// the server over stdio, runs the tool calls given as arguments in one session, prints each
// result's text, then closes the session. It runs mcp.sh, from any directory:
//
//   node tests/explore-page/mcp-cli.mjs '{"tool":"open_session","args":{"target":"desktop"}}' \
//     '{"tool":"call","args":{"tool":"locate","args":{"role":"radio","name":"Keep the draft"}}}'
import { spawn } from 'node:child_process';
import { dirname, join } from 'node:path';
import { createInterface } from 'node:readline';
import { fileURLToPath } from 'node:url';

// The same server that `claude mcp add e2e` registers: this project's e2e, with its config.
const here = dirname(fileURLToPath(import.meta.url));
const server = spawn(join(here, 'mcp.sh'), [], { stdio: ['pipe', 'pipe', 'inherit'] });
const pending = new Map();
let nextId = 1;
createInterface({ input: server.stdout }).on('line', (line) => {
  let message;
  try {
    message = JSON.parse(line);
  } catch {
    return;
  }
  const waiter = pending.get(message.id);
  if (waiter) {
    pending.delete(message.id);
    waiter(message);
  }
});

function request(method, params) {
  const id = nextId++;
  server.stdin.write(JSON.stringify({ jsonrpc: '2.0', id, method, params }) + '\n');
  return new Promise((resolve) => pending.set(id, resolve));
}

const init = await request('initialize', {
  protocolVersion: '2025-06-18',
  capabilities: {},
  clientInfo: { name: 'mcp-cli', version: '0' },
});
console.log(`server: ${JSON.stringify(init.result?.serverInfo)}`);
server.stdin.write(JSON.stringify({ jsonrpc: '2.0', method: 'notifications/initialized' }) + '\n');
const tools = await request('tools/list', {});
console.log(`tools: ${tools.result?.tools.map((tool) => tool.name).join(', ')}`);

for (const raw of process.argv.slice(2)) {
  const { tool, args } = JSON.parse(raw);
  const reply = await request('tools/call', { name: tool, arguments: args ?? {} });
  const text = reply.error
    ? `error: ${JSON.stringify(reply.error)}`
    : reply.result.content.map((part) => (part.type === 'text' ? part.text : `[${part.type}]`)).join('\n');
  console.log(`\n== ${tool} ${JSON.stringify(args ?? {})}\n${text}`);
}
await request('tools/call', { name: 'close_session', arguments: {} });
server.kill();
