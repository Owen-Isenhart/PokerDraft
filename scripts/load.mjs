/** Real WS ping + serialized-service watch receipts and table fan-out.
 * POKERDRAFT_URL=http://localhost:3000 node scripts/load.mjs 20 5
 */
import { randomUUID } from 'node:crypto';
const url=(process.env.POKERDRAFT_URL??'http://127.0.0.1:3000').replace(/^http/,'ws')+'/ws';
const count=Number(process.argv[2]??20),seconds=Number(process.argv[3]??5);
if(count<1||count>2000||seconds<1||seconds>60)throw new Error('Use 1–2000 clients for 1–60 seconds.');
const pingSamples=[],readSamples=[],sockets=[];
let messages=0,bytes=0,errors=0,finished=false;
for(let i=0;i<count;i++)await new Promise(resolve=>{
 const ws=new WebSocket(url);sockets.push(ws);const pending=new Map();let watched='';let ticks=0;
 let failed=false;
 const fail=()=>{if(!failed){errors++;failed=true;}resolve();};
 const timer=setTimeout(()=>{fail();ws.close();},10000);
 ws.onopen=()=>{clearTimeout(timer);resolve();};
 ws.onerror=()=>{clearTimeout(timer);fail();};
 ws.onclose=()=>{clearTimeout(timer);if(!finished)fail();pending.clear();};
 ws.onmessage=e=>{
  messages++;bytes+=Buffer.byteLength(String(e.data));const m=JSON.parse(String(e.data));
  if(m.type==='directory'&&m.matches.length){watched=m.matches[0].id;ws.send(JSON.stringify({op:'watch',id:randomUUID(),match_id:watched}));}
  if(m.type==='ack'&&pending.has(m.id)){const {start,kind}=pending.get(m.id);pending.delete(m.id);if(m.ok)(kind==='ping'?pingSamples:readSamples).push(performance.now()-start);else errors++;}
 };
 ws.measure=()=>{if(ws.readyState===WebSocket.OPEN){const id=randomUUID();const kind=watched&&++ticks%5===0?'watch':'ping';pending.set(id,{start:performance.now(),kind});ws.send(JSON.stringify({op:kind,id,...(kind==='watch'?{match_id:watched}:{})}));}};
});
const timer=setInterval(()=>{for(const ws of sockets)ws.measure();},100);
await new Promise(r=>setTimeout(r,seconds*1000));clearInterval(timer);for(const ws of sockets){if(ws.readyState!==WebSocket.OPEN)errors++;}finished=true;for(const ws of sockets)ws.close();
function stats(samples){samples.sort((a,b)=>a-b);const at=p=>Number((samples[Math.min(samples.length-1,Math.floor(samples.length*p))]??0).toFixed(3));return {receipts:samples.length,p50_ms:at(.5),p95_ms:at(.95),p99_ms:at(.99)};}
console.log(JSON.stringify({requested_clients:count,seconds,messages,bytes,errors,ping:stats(pingSamples),service_read:stats(readSamples)},null,2));
if(errors)process.exitCode=1;
