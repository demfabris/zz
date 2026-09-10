import { createServer } from 'node:http';
import { createHash } from 'node:crypto';

const port = Number(process.env.PORT ?? 4173);
const TOKEN = 'TOKEN-7f3a9c';
const END_MARKER = 'END-4242';
const REPORT = 'id,name,score\n1,alpha,90\n2,beta,85\n3,gamma,77\n';
const products = [
  ['Aster keyboard', 129], ['Basil mouse', 49], ['Cedar monitor', 389],
  ['Dahlia headset', 99], ['Elm webcam', 79],
];
const page = (title, body, head = '') => `<!doctype html><html><head><meta charset="utf-8"><title>${title}</title>
<style>body{font:16px system-ui;margin:2rem;max-width:60rem}nav a{margin-right:.75rem}#submenu{display:none}#menu:hover #submenu{display:block}.tall{height:120vh}</style>${head}</head>
<body><nav><a href="/">Home</a><a href="/form">Form</a><a href="/delayed">Delayed</a><a href="/dialogs">Dialogs</a><a href="/popup">Popup</a><a href="/download">Download</a><a href="/iframe">Iframe</a><a href="/shadow">Shadow</a><a href="/long">Long</a><a href="/upload">Upload</a><a href="/hover">Hover</a><a href="/keys">Keys</a><a href="/storage">Storage</a><a href="/console">Console</a><a href="/redirect">Redirect</a><a href="/spa">SPA</a><a href="/table?page=1">Table</a></nav>
<main>${body}</main></body></html>`;
const routes = {
  '/': () => page('Bench Home', `<h1>Bench Home</h1><p id="intro">Fixture site for the zz browser agent benchmark.</p>
<ul id="products">${products.map(([n, p]) => `<li class="product"><span class="name">${n}</span> <span class="price">$${p}</span></li>`).join('')}</ul>
<p data-secret="${TOKEN}" id="secret-holder">The secret lives in a data attribute.</p>
<button id="dbl" ondblclick="document.getElementById('dbl-out').textContent='double-clicked'">Double-click me</button> <span id="dbl-out"></span>`),
  '/form': () => page('Bench Form', `<h1>Order form</h1><form method="post" action="/submit" id="order">
<label>Name <input name="name" id="name" placeholder="Your name"></label><br>
<label>Email <input name="email" id="email" type="email" placeholder="you@example.com"></label><br>
<label>Role <select name="role" id="role"><option value="">Choose</option><option value="engineer">Engineer</option><option value="designer">Designer</option><option value="manager">Manager</option></select></label><br>
<label><input type="checkbox" name="newsletter" id="newsletter" value="yes"> Newsletter</label><br>
<label><input type="radio" name="plan" value="basic" id="plan-basic"> Basic</label>
<label><input type="radio" name="plan" value="pro" id="plan-pro"> Pro</label><br>
<label>Bio <textarea name="bio" id="bio"></textarea></label><br>
<button type="submit" id="submit">Place order</button></form>`),
  '/delayed': () => page('Bench Delayed', `<h1>Delayed content</h1><p id="status">Loading…</p><div id="slot"></div>
<button id="fetch">Fetch slow data</button><pre id="fetched"></pre>
<script>setTimeout(()=>{document.getElementById('status').textContent='Ready';const p=document.createElement('p');p.id='banner';p.textContent='Banner ready: ${TOKEN}';document.getElementById('slot').appendChild(p);},1500);
document.getElementById('fetch').onclick=async()=>{const r=await fetch('/api/slow');document.getElementById('fetched').textContent=await r.text();};</script>`),
  '/dialogs': () => page('Bench Dialogs', `<h1>Dialogs</h1><button id="alert" onclick="alert('Hello from alert');document.getElementById('result').textContent='alert closed'">Alert</button>
<button id="confirm" onclick="document.getElementById('result').textContent='confirmed: '+confirm('Proceed?')">Confirm</button>
<button id="prompt" onclick="document.getElementById('result').textContent='prompt: '+prompt('Your word?','none')">Prompt</button>
<p id="result">no dialog yet</p>`),
  '/popup': () => page('Bench Popup', `<h1>Popups</h1><a id="blank-link" href="/popup-child" target="_blank">Open child in new tab</a><br>
<button id="open-window" onclick="window.open('/popup-child','child')">window.open child</button>`),
  '/popup-child': () => page('Popup Child', `<h1>Popup Child</h1><p id="child-marker">child ${TOKEN}</p>`),
  '/download': () => page('Bench Download', `<h1>Download</h1><a id="report" href="/report.csv" download>Download report.csv</a>`),
  '/iframe': () => page('Bench Iframe', `<h1>Iframe host</h1><iframe id="frame" src="/iframe-child" width="400" height="200"></iframe>`),
  '/iframe-child': () => page('Iframe Child', `<button id="inner" onclick="document.getElementById('inner-out').textContent='inner clicked'">Inner button</button><p id="inner-out">inner idle</p>`),
  '/shadow': () => page('Bench Shadow', `<h1>Shadow DOM</h1><bench-widget id="widget"></bench-widget>
<script>customElements.define('bench-widget',class extends HTMLElement{connectedCallback(){const r=this.attachShadow({mode:'open'});r.innerHTML='<p id="shadow-text">Shadow says hi</p><button id="shadow-btn">Shadow button</button><p id="shadow-out">shadow idle</p>';r.getElementById('shadow-btn').onclick=()=>r.getElementById('shadow-out').textContent='shadow clicked';}});</script>`),
  '/long': () => page('Bench Long', `<h1>Long page</h1>${Array.from({ length: 200 }, (_, i) => `<p class="row">Row ${i + 1}</p>`).join('')}<p id="bottom">Bottom marker: ${END_MARKER}</p><button id="bottom-btn" onclick="this.textContent='bottom clicked'">Bottom button</button>`),
  '/upload': () => page('Bench Upload', `<h1>Upload</h1><form method="post" action="/uploaded" enctype="multipart/form-data"><input type="file" name="file" id="file"><button type="submit" id="send">Send</button></form>`),
  '/hover': () => page('Bench Hover', `<h1>Hover</h1><div id="menu"><span id="menu-label">Menu</span><div id="submenu"><a id="hidden-link" href="/hover-target">Hidden link</a></div></div>`),
  '/hover-target': () => page('Hover Target', `<h1>Hover Target</h1><p id="hover-marker">reached via hover ${TOKEN}</p>`),
  '/keys': () => page('Bench Keys', `<h1>Keyboard</h1><input id="box" placeholder="type here"><pre id="log"></pre>
<script>const log=document.getElementById('log');document.getElementById('box').addEventListener('keydown',e=>{if(e.key==='Enter')log.textContent+='ENTER\\n';if(e.key==='Escape')log.textContent+='ESC\\n';if(e.key==='a'&&(e.ctrlKey||e.metaKey))log.textContent+='SELECT-ALL\\n';});</script>`),
  '/storage': () => page('Bench Storage', `<h1>Storage</h1><button id="set" onclick="document.cookie='bench=cookie-${TOKEN}; path=/';localStorage.setItem('bench','local-${TOKEN}');render()">Set</button>
<p id="cookie"></p><p id="local"></p><script>function render(){document.getElementById('cookie').textContent='cookie: '+document.cookie;document.getElementById('local').textContent='local: '+(localStorage.getItem('bench')||'')}render();</script>`),
  '/console': () => page('Bench Console', `<h1>Console and network</h1><p id="net">waiting</p>
<script>console.error('bench-error-1');console.log('bench-log-1');fetch('/api/data').then(r=>r.json()).then(j=>{document.getElementById('net').textContent='data: '+j.value});fetch('/missing');</script>`),
  '/redirected': () => page('Redirected', `<h1>Redirected</h1><p id="redirect-marker">arrived ${TOKEN}</p>`),
  '/spa': () => page('Bench SPA', `<h1>SPA</h1><button id="go-one" onclick="history.pushState({},'', '/spa/one');document.title='SPA One';document.getElementById('view').textContent='view one'">One</button>
<button id="go-two" onclick="history.pushState({},'', '/spa/two');document.title='SPA Two';document.getElementById('view').textContent='view two'">Two</button><p id="view">view home</p>`),
  '/spa/one': () => routes['/spa'](), '/spa/two': () => routes['/spa'](),
};
const names = ['Alpha','Bravo','Charlie','Delta','Echo','Foxtrot','Golf','Hotel','India','Juliet','Kilo','Lima','Mike','November','Oscar','Papa','Quebec','Romeo','Sierra','Tango','Uniform','Victor','Whiskey','Xray','Yankee','Zulu','Omega','Sigma','Theta','Zeta'];
const send = (res, status, body, headers = {}) => { res.writeHead(status, { 'content-type': 'text/html; charset=utf-8', ...headers }); res.end(body); };
createServer((req, res) => {
  const url = new URL(req.url, `http://${req.headers.host}`);
  const path = url.pathname;
  if (req.method === 'POST' && path === '/submit') {
    let body = ''; req.on('data', c => body += c); req.on('end', () => {
      const f = new URLSearchParams(body); const fields = ['name','email','role','newsletter','plan','bio'].map(k => `${k}=${f.get(k) ?? ''}`);
      const code = 'CONF-' + createHash('sha256').update(fields.join('&')).digest('hex').slice(0, 8);
      send(res, 200, page('Order placed', `<h1>Order placed</h1><p id="code">Confirmation ${code}</p><ul>${fields.map(x => `<li>${x}</li>`).join('')}</ul>`));
    }); return;
  }
  if (req.method === 'POST' && path === '/uploaded') {
    let size = 0; let head = ''; req.on('data', c => { size += c.length; if (head.length < 600) head += c.toString('latin1', 0, 600); }); req.on('end', () => {
      const m = /filename="([^"]*)"/.exec(head); send(res, 200, page('Uploaded', `<h1>Uploaded</h1><p id="upload-result">file=${m ? m[1] : 'none'} bytes=${size}</p>`));
    }); return;
  }
  if (path === '/report.csv') return send(res, 200, REPORT, { 'content-type': 'text/csv', 'content-disposition': 'attachment; filename="report.csv"' });
  if (path === '/api/data') return send(res, 200, JSON.stringify({ value: TOKEN }), { 'content-type': 'application/json' });
  if (path === '/api/slow') return void setTimeout(() => send(res, 200, `slow ${TOKEN}`, { 'content-type': 'text/plain' }), 800);
  if (path === '/redirect') return send(res, 302, '', { location: '/redirected' });
  if (path === '/table') {
    const p = Math.min(3, Math.max(1, Number(url.searchParams.get('page') ?? 1)));
    const rows = names.slice((p - 1) * 10, p * 10).map((n, i) => `<tr><td>Item ${(p - 1) * 10 + i + 1}</td><td>${n}</td></tr>`).join('');
    return send(res, 200, page(`Bench Table ${p}`, `<h1>Table page ${p}</h1><table id="items"><tbody>${rows}</tbody></table>${p > 1 ? `<a id="prev" href="/table?page=${p - 1}">Previous</a>` : ''} ${p < 3 ? `<a id="next" href="/table?page=${p + 1}">Next</a>` : '<span id="last">last page</span>'}`));
  }
  const handler = routes[path];
  if (!handler) return send(res, 404, page('Not found', `<h1>404</h1><p>${path}</p>`));
  send(res, 200, handler());
}).listen(port, '127.0.0.1', () => console.log(`bench site on http://127.0.0.1:${port}`));
