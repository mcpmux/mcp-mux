# McpMux Security Audit

Living record of security findings and the exposure surface. Re-established
2026-07-06 as the M0 preflight for the cloud-support work
(`mcpmux.space/implementation-plans/cloud-support/00-security-preflight.md`).
A prior June 2026 audit referenced a HIGH-1 finding but was never committed;
this file replaces that gap and is now the source of truth.

---

## Findings

### HIGH-1 — Cross-Space credential exposure

- **Status: NOT REPRODUCIBLE in current code (isolation verified).**
- **Claim (June 2026):** a credential belonging to one Space could be exposed
  to another Space.
- **Audit (2026-07-06):** credential isolation is enforced structurally at two
  independent layers:
  1. **Storage.** Every credential row is keyed by `(space_id, server_id,
     credential_type)`; that tuple is the `ON CONFLICT` target and every read
     (`get`, `get_all`) filters on `space_id`
     (`crates/mcpmux-storage/src/repositories/credential_repository.rs`). A
     Space cannot read or overwrite another Space's row for the same server.
  2. **Connection pool.** Each `(space_id, server_id)` pair gets its own
     isolated `ServerInstance`
     (`crates/mcpmux-gateway/src/pool/instance.rs`), and each instance's
     `DatabaseCredentialStore` is constructed with a fixed `space_id`
     (`pool/credential_store.rs`) that it passes to every credential lookup —
     so a pooled connection can never be reused across Spaces with the wrong
     Space's credentials.
- **Regression test:** `credentials_are_isolated_across_spaces`
  (credential_repository.rs) — proves Space B cannot see Space A's credential
  for the same `server_id`, `get_all` is Space-scoped, and cross-Space writes
  don't collide. Green as of this commit.
- **Action:** if the original June audit had a concrete repro (a specific route
  or resolver path), re-file it against this section. Absent that, the finding
  is considered closed by construction + test.

---

## Exposure surface inventory

Which HTTP routes are reachable, and what gates them, per bind mode. "Loopback"
= default `127.0.0.1`; "Network" = `0.0.0.0` (opt-in `gateway.network_access`).

| Route | Loopback | Network | Gate |
|-------|----------|---------|------|
| `POST /mcp` | ✓ | ✓ | Bearer token / API key (unless `auth_disabled`, which is **forbidden on a network bind** — M0-01). Rate-limited per-peer on network binds (M0-03). |
| `GET /health` | ✓ | ✓ | none (no sensitive data) |
| `GET /.well-known/oauth-*` | ✓ | ✓ | none (metadata; 404'd when `auth_disabled`) |
| `GET /oauth/authorize`, `/authorize` | ✓ | ✓ | interactive consent completes on the host only; rate-limited |
| `POST /oauth/token`, `/oauth/register` | ✓ | ✓ | PKCE / DCR validation; rate-limited |
| `GET /oauth/clients/{id}/features` | ✓ | ✓ | public client-facing (feature list only) |
| `GET/PUT/DELETE /oauth/clients`, `/oauth/clients/{id}` | ✓ | ✗ | **loopback peer only** — client management never on the LAN (`restrict_management_to_loopback`) |
| `POST /pair/claim` | ✓ | ✓ | single-use, short-lived pairing token (mint is desktop-only); rate-limited (M0-04) |
| `GET /pair` | ✓ | ✓ | claim page; harmless without a valid token |

Cross-cutting on **network binds**:
- Inbound auth is **always required** (the `auth_disabled` convenience is
  rejected — M0-01).
- The **Host header is allowlisted** (machine IPs/hostname + `public_base_url`
  + user extras); rebinding/unknown hosts get 421 (M0-02).
- `/mcp` is **rate-limited** per (peer-IP, credential) with a 401-lockout
  damper (M0-03).

## Credential custody (unchanged, noted for completeness)

- Field-level AES-256-GCM per-token encryption; master key via DPAPI (Windows)
  / OS keychain (macOS/Linux) / file fallback (headless), `zeroize` on drop.
- TLS: the gateway speaks plain HTTP; on a network bind, treat the LAN as
  trusted or front it with a TLS-terminating proxy/tunnel. See
  `mcpmux.space/ADR/004-lan-tls-posture.md`.

## Follow-ups before broader exposure (T2/T3)

- External penetration test before any public-internet Pro GA.
- Egress/SSRF controls for cloud-hosted instances (user-configured HTTP MCP
  server URLs).
- Revisit the Host allow-any escape hatch usage in the field.
