"""Local stand-in for the Medvita server, for developing the desktop app without the Skeleton.

Serves OIDC discovery, an authorize endpoint that redirects straight back to the loopback
listener (no login page), the token endpoint with PKCE S256 checks, revocation, and the core
endpoints the shell uses (me/, organizations/, apps/active/, server/). Usage:

    py -3 tools/mock-server/mock_medvita.py      # http://127.0.0.1:8765

Never expose it beyond loopback: it signs nothing and accepts any code it issued."""
import base64, hashlib, json, time, uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlparse, parse_qs, urlencode

HOST, PORT = "127.0.0.1", 8765
ISSUER = f"http://{HOST}:{PORT}"
CODES = {}       # code -> (nonce, challenge, redirect_uri)
TOKENS = set()   # valid access tokens
ORG_A, ORG_B = str(uuid.UUID(int=1)), str(uuid.UUID(int=2))

def b64(data): return base64.urlsafe_b64encode(data).rstrip(b"=").decode()

class Handler(BaseHTTPRequestHandler):
    def log_message(self, fmt, *args):
        print(f"{self.command} {urlparse(self.path).path} -> {args[1] if len(args) > 1 else ''}", flush=True)

    def send_json(self, status, body, headers=None):
        data = json.dumps(body).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("X-Request-ID", str(uuid.uuid4()))
        for k, v in (headers or {}).items(): self.send_header(k, v)
        self.send_header("Content-Length", str(len(data)))
        self.end_headers(); self.wfile.write(data)

    def authorized(self):
        auth = self.headers.get("Authorization", "")
        return auth.startswith("Bearer ") and auth[7:] in TOKENS

    def do_GET(self):
        url = urlparse(self.path); q = parse_qs(url.query)
        if url.path == "/.well-known/openid-configuration":
            return self.send_json(200, {"issuer": ISSUER,
                "authorization_endpoint": f"{ISSUER}/identity/o/authorize",
                "token_endpoint": f"{ISSUER}/identity/o/api/token",
                "revocation_endpoint": f"{ISSUER}/identity/o/api/revoke",
                "code_challenge_methods_supported": ["S256"]})
        if url.path == "/identity/o/authorize":
            assert q["code_challenge_method"] == ["S256"] and q["client_id"] == ["medvita-desktop"]
            code = uuid.uuid4().hex
            CODES[code] = (q["nonce"][0], q["code_challenge"][0], q["redirect_uri"][0])
            self.send_response(302)
            self.send_header("Location", q["redirect_uri"][0] + "?" + urlencode({"code": code, "state": q["state"][0]}))
            self.end_headers(); return
        if url.path == "/api/v1/server/":
            return self.send_json(200, {"product": "Medvita (simulado)", "core_version": "0.1.0", "api_versions": ["v1"], "oidc_issuer": ISSUER})
        if not self.authorized():
            return self.send_json(401, {"code": "invalid_token", "detail": "Token inválido."})
        if url.path == "/api/v1/me/":
            return self.send_json(200, {"sysid": str(uuid.UUID(int=9)), "email": "ana@example.com", "first_name": "Ana", "last_name": "Souza",
                "revision": 1, "profile": {"sysid": None, "revision": 1, "display_name": "Dra. Ana Souza", "timezone": "America/Sao_Paulo", "language": "pt-br"}},
                {"ETag": '"1-1"'})
        if url.path == "/api/v1/organizations/":
            return self.send_json(200, {"results": [
                {"sysid": str(uuid.uuid4()), "organization_id": ORG_A, "is_active": True, "organization_name": "Clínica Bem Viver", "organization_slug": "bem-viver"},
                {"sysid": str(uuid.uuid4()), "organization_id": ORG_B, "is_active": True, "organization_name": "Consultório Centro", "organization_slug": "centro"}]})
        if url.path.endswith("/apps/active/"):
            return self.send_json(200, {"results": [
                {"app_id": "io.katua.sk.med.patients", "label": "med_patients", "version": "0.1.0", "title": "Pacientes"},
                {"app_id": "io.katua.sk.med.scheduling", "label": "med_scheduling", "version": "0.1.0", "title": "Agenda"}]})
        return self.send_json(404, {"code": "not_found", "detail": "Not found."})

    def do_POST(self):
        url = urlparse(self.path)
        form = parse_qs(self.rfile.read(int(self.headers.get("Content-Length", 0))).decode())
        if url.path == "/identity/o/api/token":
            grant = form["grant_type"][0]
            if grant == "authorization_code":
                nonce, challenge, redirect = CODES.pop(form["code"][0])
                if b64(hashlib.sha256(form["code_verifier"][0].encode()).digest()) != challenge or form["redirect_uri"][0] != redirect:
                    return self.send_json(400, {"error": "invalid_grant"})
            elif grant == "refresh_token":
                nonce = None
            else:
                return self.send_json(400, {"error": "unsupported_grant_type"})
            access = uuid.uuid4().hex; TOKENS.add(access)
            body = {"access_token": access, "token_type": "Bearer", "expires_in": 3600, "refresh_token": uuid.uuid4().hex}
            if nonce:
                claims = {"iss": ISSUER, "aud": "medvita-desktop", "exp": int(time.time()) + 300, "nonce": nonce, "sub": "9"}
                body["id_token"] = ".".join([b64(b'{"alg":"RS256"}'), b64(json.dumps(claims).encode()), b64(b"sig")])
            return self.send_json(200, body)
        if url.path == "/identity/o/api/revoke":
            TOKENS.discard(form.get("token", [""])[0])
            return self.send_json(200, {})
        return self.send_json(404, {"code": "not_found"})

print(f"mock Medvita on {ISSUER}", flush=True)
ThreadingHTTPServer((HOST, PORT), Handler).serve_forever()
