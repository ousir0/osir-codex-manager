import argparse, json, os, pathlib, queue, sqlite3, subprocess, tempfile, threading, hashlib
parser = argparse.ArgumentParser()
parser.add_argument("--codex", required=True, help="Absolute path to the native Codex binary (not an OpenCodex shim)")
parser.add_argument("--source-thread", help="Read-only clone of one local thread; all writes and model calls stay in the fixture")
args = parser.parse_args()
repo = pathlib.Path(__file__).resolve().parents[1]
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
root = pathlib.Path(tempfile.mkdtemp(prefix='manager-resume-probe-'))
home = root / '.codex'
home.mkdir()
(root/".manager-resume-fixture").touch()
children = []
requests = []
token = 'probe-fake-key'
source_rollout = None
source_hash = None
class Handler(BaseHTTPRequestHandler):
    def log_message(self, *args): pass
    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        requests.append({'path': self.path, 'model': body.get('model'), 'auth_matches_current': self.headers.get('Authorization') == 'Bearer ' + token})
        message_id = 'msg_probe_' + str(len(requests))
        msg = {'id':message_id, 'type':'message','role':'assistant','status':'completed','content':[{'type':'output_text','text':'OK','annotations':[]}]}
        response = {'id':'resp_probe_' + str(len(requests)),'object':'response','status':'completed','output':[msg],'usage':{'input_tokens':1,'output_tokens':1,'total_tokens':2}}
        events = [('response.created',{'response':dict(response,status='in_progress',output=[])}),('response.output_item.added',{'output_index':0,'item':dict(msg,status='in_progress',content=[])}),('response.output_text.delta',{'item_id':message_id,'output_index':0,'content_index':0,'delta':'OK'}),('response.output_item.done',{'output_index':0,'item':msg}),('response.completed',{'response':response})]
        out=''.join('event: '+typ+'\ndata: '+json.dumps(dict(data,type=typ))+'\n\n' for typ,data in events).encode()
        self.send_response(200);self.send_header('Content-Type','text/event-stream');self.send_header('Content-Length',str(len(out)));self.end_headers();self.wfile.write(out)
server=ThreadingHTTPServer(('127.0.0.1',0),Handler)
threading.Thread(target=server.serve_forever,daemon=True).start()
def config(provider):
    (home/'auth.json').write_text(json.dumps({'OPENAI_API_KEY':token,'auth_mode':'apikey'}))
    (home/'config.toml').write_text(f'model_provider = "{provider}"\nmodel = "gpt-5.4"\nopenai_base_url = "http://127.0.0.1:{server.server_port}/old/v1"\n[model_providers.opencodex]\nname = "Probe"\nbase_url = "http://127.0.0.1:{server.server_port}/new/v1"\nwire_api = "responses"\nrequires_openai_auth = true\n[features]\nshell_tool = false\n')
class Client:
    def __init__(self):
        env={k:v for k,v in os.environ.items() if not k.startswith(('CODEX','OPENAI','OPENCODEX'))}
        env.update(HOME=str(root),CODEX_HOME=str(home),OPENAI_API_KEY=token)
        self.log=(root/'stderr.log').open('a')
        self.p=subprocess.Popen([args.codex,'app-server','--stdio'],env=env,cwd=root,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=self.log,text=True)
        children.append(self.p)
        self.q=queue.Queue();self.seq=0
        threading.Thread(target=self.read,daemon=True).start()
        self.call('initialize',{'clientInfo':{'name':'resume_probe','version':'1.0'},'capabilities':{'experimentalApi':True}})
        self.send({'method':'initialized'})
    def read(self):
        for line in self.p.stdout:
            try:self.q.put(json.loads(line))
            except ValueError:pass
    def send(self,d):self.p.stdin.write(json.dumps(d)+'\n');self.p.stdin.flush()
    def call(self,method,params):
        self.seq+=1;seq=self.seq;self.send({'id':seq,'method':method,'params':params})
        while True:
            d=self.q.get(timeout=30)
            if d.get('id')==seq:
                if 'error' in d:raise RuntimeError(d['error'])
                return d['result']
    def turn(self,tid):
        self.call('turn/start',{'threadId':tid,'input':[{'type':'text','text':'Reply OK only.'}]})
        while True:
            d=self.q.get(timeout=30)
            if d.get('method')=='turn/completed':
                assert d['params']['turn']['status'] == 'completed', {'status':d['params']['turn']['status']}
                print('TURN completed',flush=True);break
    def close(self):
        self.p.terminate();self.p.wait(timeout=10);self.log.close()
try:
    if args.source_thread:
        source_home = pathlib.Path(os.environ.get('CODEX_HOME', pathlib.Path.home()/'.codex'))
        source = sqlite3.connect(f'file:{source_home}/state_5.sqlite?mode=ro', uri=True)
        row = source.execute('SELECT rollout_path FROM threads WHERE id=?',(args.source_thread,)).fetchone()
        assert row, 'Source thread not found'
        source_rollout = pathlib.Path(row[0])
        original = source_rollout.read_bytes()
        source_hash = hashlib.sha256(original).digest()
        rollout = home/'sessions'/source_rollout.name
        rollout.parent.mkdir();rollout.write_bytes(original)
        destination=sqlite3.connect(home/'state_5.sqlite');source.backup(destination);source.close()
        destination.execute('DELETE FROM threads WHERE id<>?',(args.source_thread,))
        destination.execute('UPDATE threads SET rollout_path=?,cwd=? WHERE id=?',(str(rollout),str(root),args.source_thread))
        destination.commit();destination.close()
        history_source=source_home/'thread_history_1.sqlite'
        if history_source.exists():
            source=sqlite3.connect(f'file:{history_source}?mode=ro',uri=True)
            destination=sqlite3.connect(home/'thread_history_1.sqlite')
            for name,sql in source.execute("SELECT name,sql FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'"):
                destination.execute(sql)
                columns=[row[1] for row in source.execute(f'PRAGMA table_info({name})')]
                if 'thread_id' in columns:rows=source.execute(f'SELECT * FROM {name} WHERE thread_id=?',(args.source_thread,)).fetchall()
                elif name=='_sqlx_migrations':rows=source.execute(f'SELECT * FROM {name}').fetchall()
                else:rows=[]
                if rows:destination.executemany(f'INSERT INTO {name} VALUES ('+','.join('?' for _ in columns)+')',rows)
            destination.commit();destination.close();source.close()
        tid=args.source_thread
    else:
        config('openai');c=Client()
        r=c.call('thread/start',{'cwd':str(root),'approvalPolicy':'never','sandbox':'read-only'})
        tid=r['thread']['id'];c.turn(tid);c.close()
    config('opencodex')
    def repair(mode):
        env=dict(os.environ, MANAGER_RESUME_FIXTURE=str(root), MANAGER_RESUME_TARGET=mode, CARGO_INCREMENTAL='0')
        result=subprocess.run(['cargo','test','--manifest-path',str(repo/'src-tauri/Cargo.toml'),'real_codex_fixture','--lib','--locked','--','--ignored'],env=env,capture_output=True,text=True,timeout=180)
        assert result.returncode == 0 and '1 passed' in result.stdout, result.stdout+result.stderr
    repair('opencodex')
    for provider in ['opencodex','openai','opencodex']:
        token = 'probe-relogin-key' if provider == 'openai' else 'probe-fake-key'
        config(provider)
        repair('default' if provider == 'openai' else 'opencodex')
        for i in range(2):
            c=Client()
            r=c.call('thread/resume',{'threadId':tid,'cwd':str(root),'approvalPolicy':'never','sandbox':'read-only'})
            assert r.get('modelProvider') == provider, {'expected':provider,'actual':r.get('modelProvider'),'model':r.get('model')}
            print('RESUME',provider,i,r.get('model'),flush=True)
            c.turn(tid);c.close()
            expected='/old/v1/responses' if provider == 'openai' else '/new/v1/responses'
            assert requests[-1]['path'] == expected, requests
            assert requests[-1]['auth_matches_current'], 'Resumed turn used stale credentials'
    print('REQUESTS',requests,flush=True)
finally:
    for child in children:
        if child.poll() is None:
            child.terminate();child.wait(timeout=10)
    print('ARTIFACTS',root,flush=True)
    server.shutdown()
    if source_rollout is not None:
        assert hashlib.sha256(source_rollout.read_bytes()).digest() == source_hash, 'Source rollout changed during the read-only probe'
