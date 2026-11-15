import json
import socket
import threading
import unittest
from http.server import BaseHTTPRequestHandler, HTTPServer
from unittest.mock import patch
from pokerdraft import Client, ProtocolError, arena_url, http_command

class TransportTest(unittest.TestCase):
    def frames(self, data):
        left,right=socket.socketpair()
        self.addCleanup(left.close);self.addCleanup(right.close)
        c=Client.__new__(Client);c.socket=left;c.buffer=bytearray(data);c.deadline=None;c.timeout=.1
        return c,right
    def test_invalid_control_and_server_frames(self):
        for data in [b'\x09\x00',b'\x89\x7e\x00\x7e',b'\x81\x82',b'\xc1\x00',b'\x81\x02[]']:
            c,_=self.frames(data)
            with self.assertRaises(ProtocolError):c._receive()
    def test_fragmented_text_with_interleaved_ping(self):
        c,peer=self.frames(b'\x01\x05{"ok"\x89\x01x\x80\x06:true}')
        self.assertEqual(c._receive(),{'ok':True})
        self.assertEqual(peer.recv(100)[0],0x8a)
    def test_transport_policy(self):
        for url in ['http://arena.example','ws://arena.example','https://user:password@arena.example','https://arena.example/?token=secret']:
            with self.assertRaises(ValueError):arena_url(url)
        self.assertEqual(arena_url('http://localhost:3000').hostname,'localhost')
        self.assertEqual(arena_url('https://arena.example').hostname,'arena.example')
        with patch.dict('os.environ',{'POKERDRAFT_ALLOW_INSECURE':'true'}):
            self.assertEqual(arena_url('http://192.168.1.2').hostname,'192.168.1.2')
    def test_http_does_not_forward_authorization_on_redirect(self):
        hits=[]
        class Handler(BaseHTTPRequestHandler):
            def log_message(self,*args):pass
            def do_POST(self):
                self.send_response(302);self.send_header('Location','/leaked');self.end_headers()
            def do_GET(self):
                hits.append(self.headers.get('Authorization'));self.send_response(200);self.end_headers();self.wfile.write(json.dumps({'ok':True,'result':{}}).encode())
        server=HTTPServer(('127.0.0.1',0),Handler)
        worker=threading.Thread(target=server.serve_forever,daemon=True);worker.start()
        try:
            with self.assertRaisesRegex(ProtocolError,'redirect refused'):
                http_command(f'http://127.0.0.1:{server.server_port}','account','test-only-not-a-real-secret')
            self.assertEqual(hits,[])
        finally:server.shutdown();server.server_close();worker.join()

if __name__=='__main__':unittest.main()
