// Offline check of the broker's request handling: `node broker/test.mjs`.
import assert from "node:assert/strict";
import worker from "./src/index.js";

const env = { WHOOP_CLIENT_ID: "id-123", WHOOP_CLIENT_SECRET: "secret-xyz" };
let sent = null;
globalThis.fetch = async (url, init) => {
  sent = { url, body: new URLSearchParams(init.body.toString()) };
  return new Response('{"access_token":"a","refresh_token":"r","expires_in":3600}', { status: 200 });
};
const post = (path, fields, e = env) =>
  worker.fetch(new Request(`https://broker.example${path}`, { method: "POST", body: new URLSearchParams(fields) }), e);

// Code exchange: secret is added, PKCE verifier is forwarded.
let res = await post("/oauth/token", {
  grant_type: "authorization_code", code: "abc", code_verifier: "ver",
  redirect_uri: "http://localhost:8765/callback",
});
assert.equal(res.status, 200);
assert.equal((await res.json()).access_token, "a");
assert.equal(sent.url, "https://api.prod.whoop.com/oauth/oauth2/token");
assert.equal(sent.body.get("client_secret"), "secret-xyz");
assert.equal(sent.body.get("client_id"), "id-123");
assert.equal(sent.body.get("code_verifier"), "ver");

// Refresh.
res = await post("/oauth/token", { grant_type: "refresh_token", refresh_token: "r1" });
assert.equal(res.status, 200);
assert.equal(sent.body.get("refresh_token"), "r1");
assert.equal(sent.body.get("scope"), "offline");

// Callers can't override the credentials or redirect elsewhere.
sent = null;
res = await post("/oauth/token", {
  grant_type: "authorization_code", code: "abc", redirect_uri: "https://evil.example/cb",
});
assert.equal(res.status, 400);
assert.equal(sent, null);
res = await post("/oauth/token", { grant_type: "refresh_token", refresh_token: "r", client_id: "other" });
assert.equal(sent.body.get("client_id"), "id-123");

// Everything else is rejected without calling WHOOP.
sent = null;
assert.equal((await post("/oauth/token", { grant_type: "client_credentials" })).status, 400);
assert.equal((await post("/other", {})).status, 404);
assert.equal((await worker.fetch(new Request("https://broker.example/oauth/token"), env)).status, 405);
assert.equal((await post("/oauth/token", { grant_type: "refresh_token", refresh_token: "r" }, {})).status, 500);
assert.equal(sent, null);

console.log("broker: all checks passed");
