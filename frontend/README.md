# Browser foundation

Storefront (Astro/React) and Admin (React/Vite) build independently. Commerce data
will come from live `/api/v1` APIs as capabilities arrive. T01 exposes only the
authenticated `GET /api/v1/admin/status` resource, `{status:"available",capabilities:[]}`.
It verifies admin authority and does not assert complete M1 commerce readiness.
Navigation names upcoming capabilities without synthesizing commerce data.

Use Node **24.19.0** and npm **11.18.0** (also pinned in package files and Docker):

```sh
npm ci --prefix frontend/storefront
npm run typecheck --prefix frontend/storefront
npm run test --prefix frontend/storefront
npm run build --prefix frontend/storefront
npm ci --prefix frontend/admin
npm run typecheck --prefix frontend/admin
npm run test --prefix frontend/admin
npm run build --prefix frontend/admin
npm ci --prefix frontend/e2e
npx --prefix frontend/e2e playwright install --with-deps chromium
# BC_TEST_ADMIN_DATABASE_URL must reference an isolated operations PostgreSQL.
cargo test -p better-commerce-core --test browser_live --locked -- --ignored --nocapture
```

The browser fixture provisions and cleans up its own scoped database/role. It
serves both production builds through Rust/Axum and checks live readiness, Astro
hydration, enforced CSP, token verification, navigation, empty storage, token
forget/reload, secret-free URLs/assets, and desktop/mobile layout. Screenshots
are ignored under `output/playwright`. For local checks with an already installed
Chrome, set `BC_BROWSER_CHANNEL=chrome`; CI uses pinned Playwright Chromium.
On Windows use `deploy/with-test-postgres.ps1` around the Cargo command, with
`BC_POSTGRES_BIN` set to your PostgreSQL 18 binary directory.

## Enable browser delivery

Add an explicit HTTP configuration to the installation manifest:

```yaml
http:
  public_origin: http://127.0.0.1:3000
  admin_token: { env: BC_INSTALLATION_ADMIN_TOKEN }
  # Alternatively: { file: ./secrets/admin-token }
```

Generate the token from at least **32 CSPRNG bytes**, encoded as unpadded base64url.
For example, write `secrets.token_urlsafe(32)` from Python to an ignored secret
file; never place the value in a manifest or shell command. The server checks
canonical base64url encoding and decoded length (32–128 bytes); generation is the
operator's responsibility because randomness cannot be inferred from a string.
The verifier uses SHA-256 and constant-time comparison. Rotating the configured
secret takes effect on restart. Missing/unreadable/malformed enabled configuration
fails startup and reconciliation before installation mutation. Omitting `http`
preserves the existing M0 profile and serves no browser assets/admin authority.

Direct local HTTP requires both a loopback origin and loopback peer. Production
uses a canonical HTTPS `public_origin` and one explicitly configured
`trusted_proxy_ip`. Terminate TLS at that reverse proxy, preserve `Host`, overwrite
`X-Forwarded-Proto` to `https`, and restrict access to the API listener. Forwarded
headers from other peers cannot grant HTTPS status. Browser mutations require
the exact configured Origin; authenticated non-browser clients can omit Origin.
No permissive CORS or cookie/session authority is introduced.

The local reconciler resolves environment/file references on the host and passes
the admin value only through the API container's `BC_RESOLVED_ADMIN_TOKEN`
environment. For the local loopback-published Compose port, reconciliation
discovers the private network gateway and supplies `BC_HTTP_LOOPBACK_PROXY_IP`.
Only that trusted ingress peer or actual loopback may carry local HTTP credentials;
other container/network peers are rejected. This exception relies on the checked-in
loopback-only port publication. Production containers use the trusted HTTPS ingress
above. Do not set the local ingress override on a publicly exposed listener.
`STOREFRONT_DIST` and `ADMIN_DIST` select asset directories (defaults are the two
frontend `dist` directories; the image uses `/app/storefront` and `/app/admin`).
Assets and the Astro CSP hash manifest must exist before enabled startup.

The storefront build hashes actual emitted inline hydration scripts/styles.
Rust combines these hashes with a self-only CSP, without `unsafe-inline` or
`unsafe-eval`. Admin uses external scripts/styles. Both surfaces have no remote
scripts, analytics, frontend commerce backend, or static commerce snapshots.
Admin tokens live only in React/form memory and explicit authorization headers;
reload and “Forget token” require entry again. Authorized responses use no-store.

Shared HTTP encodings live in `core/src/http/wire.rs`; browser arithmetic helpers
live in `frontend/shared/wire.ts`. Endpoint DTOs must deny unknown fields and use
`StrictJson`, which also rejects nested duplicates and enforces stream/media
limits. Owners decode opaque IDs and interpret cursors, and map checked decimal
range failures to their specific 422 code. Quantities use numeric input with
explicit positive-i32 validation. Success resources are direct; lists use
`{items,next_cursor}`. Request IDs appear on every response; new M1 errors carry
code/message/details/request_id. M0 lowercase error bodies remain intact.
Secret-returning endpoints must extract `CredentialTransport` and return
`SecretJson`, requiring secure ingress before creation and adding no-store even
when no bearer exists yet. The real Compose smoke also enables this HTTP profile
and checks production asset delivery and authenticated admin status.
