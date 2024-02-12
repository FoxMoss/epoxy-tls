from http.server import HTTPServer, SimpleHTTPRequestHandler, test
import sys

class RequestHandler (SimpleHTTPRequestHandler):
    def end_headers (self):
        self.send_header('Cross-Origin-Opener-Policy', 'same-origin')
        self.send_header('Cross-Origin-Embedder-Policy', 'require-corp')
        self.send_header('Cross-Origin-Allow-Origin', '*')
        SimpleHTTPRequestHandler.end_headers(self)

test(RequestHandler, HTTPServer, port=int(sys.argv[1]) if len(sys.argv) > 1 else 8000)
