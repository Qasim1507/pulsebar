// PulseBar token broker (Cloudflare Worker).
//
// A desktop app can't keep an OAuth client secret: anyone could extract it
// from the download. So PulseBar sends its token requests here, and this
// Worker adds the secret and forwards them to WHOOP. It is stateless: it
// stores nothing and logs nothing, and it never sees heart rate or recovery
// data — only the short-lived authorization code and the tokens passing
// through.

const WHOOP_TOKEN_URL = "https://api.prod.whoop.com/oauth/oauth2/token";
// The only redirect the desktop app ever uses (see src-tauri/src/whoop.rs).
const REDIRECT_URI = "http://localhost:8765/callback";

const json = (status, body) =>
  new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json", "cache-control": "no-store" },
  });

export default {
  async fetch(request, env) {
    const { pathname } = new URL(request.url);
    if (pathname !== "/oauth/token") return json(404, { error: "not_found" });
    if (request.method !== "POST") return json(405, { error: "method_not_allowed" });
    if (!env.WHOOP_CLIENT_ID || !env.WHOOP_CLIENT_SECRET) {
      return json(500, { error: "broker_not_configured" });
    }

    let form;
    try {
      form = await request.formData();
    } catch {
      return json(400, { error: "invalid_request" });
    }
    const field = (name) => {
      const value = form.get(name);
      return typeof value === "string" && value.length > 0 && value.length < 4096 ? value : null;
    };

    const upstream = new URLSearchParams({
      client_id: env.WHOOP_CLIENT_ID,
      client_secret: env.WHOOP_CLIENT_SECRET,
    });

    const grant = field("grant_type");
    if (grant === "authorization_code") {
      const code = field("code");
      if (!code || field("redirect_uri") !== REDIRECT_URI) return json(400, { error: "invalid_request" });
      upstream.set("grant_type", grant);
      upstream.set("code", code);
      upstream.set("redirect_uri", REDIRECT_URI);
      const verifier = field("code_verifier");
      if (verifier) upstream.set("code_verifier", verifier);
    } else if (grant === "refresh_token") {
      const refreshToken = field("refresh_token");
      if (!refreshToken) return json(400, { error: "invalid_request" });
      upstream.set("grant_type", grant);
      upstream.set("refresh_token", refreshToken);
      upstream.set("scope", "offline");
    } else {
      return json(400, { error: "unsupported_grant_type" });
    }

    let response;
    try {
      response = await fetch(WHOOP_TOKEN_URL, {
        method: "POST",
        headers: { "content-type": "application/x-www-form-urlencoded" },
        body: upstream,
      });
    } catch {
      return json(502, { error: "whoop_unreachable" });
    }
    // Pass WHOOP's answer straight through, success or error.
    return new Response(await response.text(), {
      status: response.status,
      headers: { "content-type": "application/json", "cache-control": "no-store" },
    });
  },
};
