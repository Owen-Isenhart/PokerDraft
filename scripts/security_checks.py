#!/usr/bin/env python3
"""Real HTTP/WS security regression tests against an isolated local server."""
import json,os,signal,subprocess,sys,tempfile,time,urllib.request,urllib.error,uuid
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'sdk'/'python'))
from pokerdraft import Client,ProtocolError
URL='http://127.0.0.1:3111'
def call(path='/api/command',body=None,token='',headers=None):
    h={'Content-Type':'application/json',**(headers or {})}
    if token:h['Authorization']='Bearer '+token
    req=urllib.request.Request(URL+path,data=json.dumps(body).encode() if body is not None else None,headers=h)
    try:r=urllib.request.urlopen(req,timeout=5)
    except urllib.error.HTTPError as error:r=error
    with r:
        raw=r.read();
        try:value=json.loads(raw)
        except ValueError:value={'text':raw.decode(errors='replace')}
        return r.status,r.headers,value

def cmd(op,token='',**fields):return call(body={'id':str(uuid.uuid4()),'op':op,**fields},token=token)
def start(db,port=3111):
    env=dict(os.environ,POKERDRAFT_DB=str(db),POKERDRAFT_BIND=f'127.0.0.1:{port}',POKERDRAFT_AUCTION_MS='250',RUST_LOG='error',POKERDRAFT_METRICS_TOKEN='isolated-metrics-fixture-token-123456789')
    return subprocess.Popen([str(ROOT/'target/release/pokerdraft-server')],cwd=ROOT,env=env,stdout=subprocess.DEVNULL,stderr=subprocess.PIPE)
def main():
 with tempfile.TemporaryDirectory(prefix='pokerdraft-security-') as directory:
    db=Path(directory)/'db.sqlite';p=start(db);q=None;c=None
    try:
        for _ in range(100):
            try:call('/healthz');break
            except OSError:time.sleep(.05)
        assert p.poll() is None
        public_health=call('/healthz')[2]['result']
        assert 'commands' not in public_health and 'rejected' not in public_health
        assert call('/metrics')[0]==401 and call('/api/metrics')[0]==404
        assert call('/metrics',token='isolated-metrics-fixture-token-123456789')[0]==200
        for h in [{'Host':'attacker.example','Origin':'http://attacker.example'},{'Origin':'null'},{'Origin':'https://attacker.example'}]:assert call('/api/directory',headers=h)[0]==403
        assert call('/api/matches?offset=18446744073709551615')[0]==400
        assert call(body={'op':'ping','id':{'x':1}})[0]==400
        c=Client(URL)
        for i in range(6):
            try:c.command('create_team',name=f'WS forbidden {i}');raise AssertionError('WS registration bypass')
            except ProtocolError as error:assert str(error)=='transport_not_allowed'
        assert call('/api/directory')[2]['result']['teams']==0
        owners=[]
        for i in range(6):
            status,headers,r=cmd('create_team',name=f'Security Team {i}')
            assert status==(200 if i<5 else 429)
            if i<5:
                owners.append(r['result']['owner_token']);assert headers['Cache-Control']=='no-store'
        status,_,r=cmd('create_bot',owners[0],name='v1');assert status==200;bot=r['result']
        c.command('auth',token=bot['bot_token'])
        try:c.command('auth',token='incorrect');raise AssertionError('bad auth accepted')
        except ProtocolError:pass
        try:c.command('join');raise AssertionError('failed auth retained former role')
        except ProtocolError as e:assert str(e)=='bot_auth_required'
        assert cmd('rotate_bot',owners[1],bot_id=bot['bot']['id'])[0]==404
        cmd('rotate_bot',owners[0],bot_id=bot['bot']['id'])
        try:c.command('auth',token=bot['bot_token']);raise AssertionError('revoked credential accepted')
        except ProtocolError as e:assert str(e)=='bot_auth_required'
        q=start(db,3112);q.wait(timeout=5);assert q.returncode!=0
        live=call('/api/directory')[2]['result']['matches'];assert live
        record=call('/api/matches/'+live[0]['id'])[2]['result'];assert record['status']=='live' and record['snapshot']['seed'] is None
        assert 'private_seed' not in json.dumps(record)
        if os.name=='posix':assert db.stat().st_mode&0o777==0o600
        print('PASS: strict host/origin, request schema, WS transport isolation, registration quota, no-store secrets, auth clearing/rotation, cross-team authorization, exclusive database ownership and live seed privacy',flush=True)
    finally:
        if c:c.close()
        for process in [q,p]:
            if process and process.poll() is None:
                process.send_signal(signal.SIGTERM)
                try:process.wait(timeout=5)
                except subprocess.TimeoutExpired:process.kill();process.wait(timeout=5)
if __name__=='__main__':main()
