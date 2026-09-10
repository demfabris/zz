// mcp-call.mjs "<server command>" list | call <tool> '<json args>' [...more call pairs]
// Minimal MCP stdio client: starts the server, initializes, then lists tools or calls them in order.
import { spawn } from 'node:child_process';
const [serverCommand, ...rest] = process.argv.slice(2);
if (!serverCommand) { console.error('usage: mcp-call.mjs "<server command>" list | call <tool> <json> ...'); process.exit(2); }
const child = spawn('sh', ['-c', serverCommand], { stdio: ['pipe', 'pipe', 'inherit'] });
let seq = 0; const pending = new Map(); let buffer = '';
child.stdout.on('data', chunk => {
  buffer += chunk.toString();
  let index;
  while ((index = buffer.indexOf('\n')) >= 0) {
    const line = buffer.slice(0, index).trim(); buffer = buffer.slice(index + 1);
    if (!line) continue;
    let message; try { message = JSON.parse(line); } catch { continue; }
    if (message.id !== undefined && pending.has(message.id)) { const { ok, no } = pending.get(message.id); pending.delete(message.id); message.error ? no(new Error(JSON.stringify(message.error))) : ok(message.result); }
  }
});
const request = (method, params = {}) => new Promise((ok, no) => {
  const id = ++seq; pending.set(id, { ok, no });
  child.stdin.write(JSON.stringify({ jsonrpc: '2.0', id, method, params }) + '\n');
  setTimeout(() => { if (pending.has(id)) { pending.delete(id); no(new Error(`timeout: ${method}`)); } }, Number(process.env.MCP_TIMEOUT_MS ?? 30000)).unref();
});
const notify = (method, params = {}) => child.stdin.write(JSON.stringify({ jsonrpc: '2.0', method, params }) + '\n');
const textOf = result => (result?.content ?? []).map(part => part.type === 'text' ? part.text : `[${part.type}${part.mimeType ? ' ' + part.mimeType : ''} ${part.data ? part.data.length + ' bytes' : ''}]`).join('\n');
try {
  await request('initialize', { protocolVersion: '2025-06-18', capabilities: {}, clientInfo: { name: 'zz-bench', version: '0' } });
  notify('notifications/initialized');
  let i = 0; let last = '';
  const substitute = raw => raw.replace(/\{\{last:(.*?)\}\}/g, (_, pattern) => { const match = new RegExp(pattern).exec(last); return match ? (match[1] ?? match[0]) : ''; });
  while (i < rest.length) {
    if (rest[i] === 'list') { const { tools } = await request('tools/list'); console.log(tools.map(t => t.name).sort().join('\n')); i += 1; }
    else if (rest[i] === 'call') { const name = rest[i + 1]; const args = rest[i + 2] ? JSON.parse(substitute(rest[i + 2])) : {}; const started = performance.now(); const result = await request('tools/call', { name, arguments: args }); last = textOf(result); console.log(`## ${name} (${(performance.now() - started).toFixed(0)} ms)${result.isError ? ' ERROR' : ''}\n${last}`); i += 3; }
    else { console.error(`unknown word ${rest[i]}`); process.exit(2); }
  }
} catch (error) { console.error(String(error.message ?? error)); process.exitCode = 1; }
finally { child.stdin.end(); child.kill(); }
