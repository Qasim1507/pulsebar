# PulseBar token broker

A tiny Cloudflare Worker that lets PulseBar users sign in to WHOOP without
creating their own developer keys.

A downloadable app can't hide an OAuth client secret, so the app sends its two
token requests (code exchange and refresh) here; the Worker adds the secret
and forwards them to WHOOP. It stores nothing, logs nothing, and never sees
health data. All WHOOP data requests still go straight from the app to WHOOP.

## Deploy (one time, about 10 minutes)

You need a free Cloudflare account and the Client ID and Secret of the WHOOP
developer app that all PulseBar users will share.

1. Put your WHOOP **Client ID** in `wrangler.toml` (`WHOOP_CLIENT_ID = "..."`).
   It is not secret.
2. From this folder, sign in and deploy:

   ```bash
   npx wrangler login
   npx wrangler deploy
   ```

   The deploy prints the Worker's URL, like
   `https://pulsebar-whoop-broker.<your-subdomain>.workers.dev`.
3. Store the **Client Secret** on Cloudflare (it is typed into the prompt and
   never written to this repo):

   ```bash
   npx wrangler secret put WHOOP_CLIENT_SECRET
   ```

4. In `src-tauri/src/whoop.rs`, fill in `SHARED_CLIENT_ID` and `BROKER_URL`
   with the Client ID and the Worker URL, then rebuild and release the app.

From then on Settings shows a single **Connect WHOOP** button. People who
prefer their own keys can still enter them under "Use my own WHOOP developer
keys".

## Before sharing widely

- The WHOOP developer app must list `http://localhost:8765/callback` as a
  Redirect URL and have the scopes from the main README.
- WHOOP limits how many members can use an app until WHOOP has reviewed it.
  Request approval in the developer dashboard before promoting the app.
- WHOOP's API terms restrict use of the WHOOP name and logo; describe the app
  as "works with WHOOP" rather than branding it as a WHOOP product.

## Test

```bash
node test.mjs
```

Runs the request handling offline against a fake WHOOP endpoint.
