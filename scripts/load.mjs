/** Measure real WS ping receipts and selected-table fan-out. No dependencies.
 * POKERDRAFT_URL=http://localhost:3000 node scripts/load.mjs 100 5
 */
import {randomUUID} from 'node:crypto';
const url=(process.env.POKERDRAFT_URL??'http://127.0.0.1:3000').replace(/^http/,'ws')+'/ws';
const count=Number(process.argv[2]??20),seconds=Number(process.argv[3]??5);
if(count<1||count>2000||seconds<1||seconds>60)throw new Error('Use 1–2000 clients for 1–60 seconds.');
const samples=[],sockets=[];let messages=0,bytes=0,errors=0;
const open=async()=>{for(let i=0;i<count;i++)await new Promise((resolve,reject)=>{const ws=new WebSocket(url);sockets.push(ws);const pending=new Map();ws.onopen=()=>{resolve();};ws.onerror=()=>{errors++;resolve();};ws.onmessage=e=>{messages++;bytes+=Buffer.byteLength(String(e.data));const m=JSON.parse(String(e.data));if(m.type==='directory'&&m.matches.length)ws.send(JSON.stringify({op:'watch',id:randomUUID(),match_id:m.matches[0].id}));if(m.type==='ack'&&pending.has(m.id)){samples.push(performance.now()-pending.get(m.id));pending.delete(m.id);}};ws.pingReceipt=()=>{if(ws.readyState===WebSocket.OPEN){const id=randomUUID();pending.set(id,performance.now());ws.send(JSON.stringify({op:'ping',id}));}};});};
await open();const timer=setInterval(()=>{for(const ws of sockets)ws.pingReceipt();},100);await new Promise(r=>setTimeout(r,seconds*1000));clearInterval(timer);for(const ws of sockets)ws.close();samples.sort((a,b)=>a-b);const percentile=p=>Number((samples[Math.min(samples.length-1,Math.floor(samples.length*p))]??0).toFixed(3));console.log(JSON.stringify({requested_clients:count,seconds,receipts:samples.length,messages,bytes,errors,p50_ms:percentile(.5),p95_ms:percentile(.95),p99_ms:percentile(.99)},null,2));
