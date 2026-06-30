# Per-Device Machine Identity Header

**Last Updated:** Jun 30, 2026
**Status:** Implemented (Jun 30, 2026)
**Branch:** `feat/workspace-machine-binding`
**Depends on:** `workspace-machine-binding.md`, `deny-by-default-bindable-callers.md`
**Unblocks:** Machine-scoped bindings work when multiple physical devices share one tunneled gateway

---

## Problem

When Cursor on Rohan reaches the gateway on Gondor via a shared tunnel (`gateway.public_url`), the resolver cannot tell which physical device made the request. Tier 1 falls back to `inbound_clients.machine_id` (static, set once at OAuth consent) and then `gateway.local_machine_id` (always Gondor). A path bound only on Gondor matches even when the caller is on Rohan.

## Decision

Add optional per-request header `X-Mcpmux-Machine-Id: <machine-uuid>` in each device's MCP client config. Gateway reads it as the highest-priority machine signal in Tier 1 binding lookup.

## Resolver priority (Tier 1 machine scoping)

**Header absent** (unchanged):

```text
1. inbound_clients.machine_id (static OAuth client tag)
2. gateway.local_machine_id (this install)
3. global binding (machine_id IS NULL)
```

**Header present** (`X-Mcpmux-Machine-Id` with valid UUID):

```text
1. Header machine id only
2. global binding (machine_id IS NULL)
```

Client and gateway-local machine tags are skipped when the header is set, so a tunneled Rohan caller is not mistaken for Gondor.

## Files modified

| File | Change |
| ---- | ------ |
| `crates/mcpmux-gateway/src/mcp/context.rs` | `OAuthContext.request_machine_id`; parse header |
| `crates/mcpmux-gateway/src/services/feature_set_resolver.rs` | `resolve(..., request_machine_id)`; header-first lookup |
| `crates/mcpmux-gateway/src/services/authorization.rs` | Forward `request_machine_id` |
| `crates/mcpmux-gateway/src/mcp/handler.rs` | Thread through routing + binding prompts |
| `tests/rust/tests/integration/feature_set_resolver.rs` | Header outranks client/local; deny when only other machine bound |
| `apps/desktop/src/features/settings/SettingsPage.tsx` | Copy MCP header snippet per machine; MachineIdSection on viewer card |
| `apps/desktop/src/components/ViewerIdentity.tsx` | MachineIdSection in viewer modal (status bar → edit) |
| `apps/desktop/src/components/machine-id-section.component.tsx` | Shared machine ID display, dual copy, paste-to-link |
| `apps/desktop/src/lib/machine-id.helpers.ts` | UUID validation, MCP header snippet builder, clipboard helper |
| `apps/desktop/src/hooks/use-viewer-identity.hook.tsx` | `linkMachineById` for paste-to-link existing catalog rows |
| `apps/desktop/src/locales/en/common.json` | Viewer modal machine ID + copy/link strings |
| `apps/desktop/src/locales/en/settings.json` | Copy header + copy UUID toast strings |
| `crates/mcpmux-gateway/src/services/meta_tools/meta_tool_common.rs` | Pass `None` for header (meta tools have no HTTP context) |
| `crates/mcpmux-gateway/src/services/meta_tools/set_workspace_root.rs` | Pass `None` for header |
| `crates/mcpmux-gateway/src/consumers/mcp_notifier.rs` | Pass `None` for header (session fan-out) |
| `docs/guide/remote-access.mdx` | Example config with optional machine header |

## Implementation notes

- Malformed header values are ignored; full client → local → global chain applies.
- When header is present (valid UUID), client and gateway-local machine tags are **not** consulted.
- Tests: `request_machine_header_outranks_client_and_local_machine`, `request_machine_header_enables_deny_when_only_other_machine_bound`.
- Validated Jun 30, 2026: 331 integration tests, clippy clean, desktop typecheck clean.

## Client setup

On each physical device, add to that device's MCP client config (alongside Cloudflare Access headers if used):

```json
{
  "headers": {
    "X-Mcpmux-Machine-Id": "<machine-uuid-from-settings>"
  }
}
```

Copy the snippet from **Settings → Machine Identity** (viewer card or **Manage all machines**) or from the **status bar viewer modal** (click `Viewer: …` → **Copy MCP header**). **Copy UUID** is also available on all three surfaces.

## See also

- [Remote Access](/docs/remote-access/) — tunneled MCP client config (CF Access + machine header)
- [Workspaces](/docs/workspaces/) — machine-scoped bindings
- [Clients](/docs/clients/) — multi-device tunnel setup
- [workspace-machine-binding.md](./workspace-machine-binding.md) — machine catalog and binding model

```bash
pnpm test:rust
pnpm typecheck
pnpm lint
```
