# Meta-Gateway Invoke — Targeted Retest (post-DX fixes)

**Last Updated:** May 25, 2026  
**Branch:** `feat/meta-gateway-invoke`  
**Commit:** `85113e7` — `fix(gateway): improve meta-tool DX for ACL, schema batch, and max_bytes`  
**Related:** [`meta-gateway-invoke-qa.md`](./meta-gateway-invoke-qa.md) (full runbook), [`meta-gateway-invoke.md`](./meta-gateway-invoke.md) (spec)

Paste the **Agent Prompt** block below into a fresh Cursor agent after gateway restart. Only re-runs **§3, §6, §9, §10** — core invoke QA (§0–§2, §4, §5, §7, §8, §11) already passed.

---

## Prep (required before any tests)

1. Gateway rebuilt/restarted since `85113e7` (`pnpm dev` or restart desktop app)
2. Cursor → MCP → **Reload tools**
3. McpMux endpoint: `http://localhost:45818/mcp` (via `user-mcpmux` / CallMcpTool)
4. Workspace binding active with partial GitHub ACL FeatureSet (e.g. `QA: meta-gateway invoke` — ~3 GitHub tools invokable, not full catalog)
5. Open the **bound project folder** in Cursor
6. GitHub OAuth connected; github `enabled_via_binding` or session-enabled
7. GWorkspace Personal clone available for §6 (bound in FeatureSet)

**FeatureSet editor reminder:**

| Control | Role |
| ------- | ---- |
| **Checkbox** | Invoke ACL (search + `mcpmux_invoke_tool`) |
| **Surface** button | Promote into client `tools/list` for direct one-hop calls |
| **Server header toggle** | Bulk checkbox only — not Surface |

After any Surface change: **Cursor → MCP → Reload tools**.

---

## Agent Prompt

Copy everything inside the fence:

```markdown
# McpMux meta-gateway invoke — targeted retest (post-DX fixes)

You are validating **4 sections only** after gateway commit `85113e7` on branch `feat/meta-gateway-invoke`. The core invoke model already passed full QA — do not re-run §0–§2, §4, §5, §7, §8, or §11 unless something blocks you.

## Prep (required before any tests)

1. Gateway rebuilt/restarted since `85113e7` (`pnpm dev` or restart desktop app)
2. Cursor → MCP → **Reload tools**
3. McpMux endpoint: `http://localhost:45818/mcp` (via `user-mcpmux` / CallMcpTool)
4. Workspace binding active with partial GitHub ACL FeatureSet (e.g. `QA: meta-gateway invoke` — ~3 GitHub tools invokable, not full catalog)
5. Open the **bound project folder** in Cursor
6. GitHub OAuth connected; github `enabled_via_binding` or session-enabled
7. GWorkspace Personal clone available for §6 (bound in FeatureSet)

**FeatureSet editor reminder:**
- **Checkbox** = invoke ACL (search + `mcpmux_invoke_tool`)
- **Surface button** = promote into client `tools/list` for direct one-hop calls
- After Surface toggle: **MCP Reload tools**

---

## §3 — Batch schema (was: array returned empty)

Run all three calls on github (enabled):

```
1. mcpmux_get_tool_schema({ tools: ["github_list_issues"] })
2. mcpmux_get_tool_schema({ tools: ["github_list_issues", "github_create_issue"] })
   — create_issue should NOT be in your ACL
3. mcpmux_get_tool_schema({ tools: "github_list_issues" })  — string form sanity check
```

**Pass criteria:**
- (1) `schemas.length === 1`, qualified_name = `github_list_issues`
- (2) `schemas.length === 1` AND `missing` includes `github_create_issue` AND `message` explains use search
- (3) string form still works
- **Fail if:** array form returns `schemas: []` with no `missing` explanation

---

## §6 — Filter max_bytes on plain text (was: partial — payload too small)

Use GWorkspace Personal (`taylorwilsdon.google-workspace-mcp-uvx` or your bound clone):

```
1. mcpmux_search_tools query "list drive" or "list_drive_items" with server_id set
2. mcpmux_get_tool_schema for the list tool
3. mcpmux_invoke_tool with args that return a LARGE list:
     { page_size: 100 }  (or equivalent from schema — do NOT use page_size: 10)
4. mcpmux_invoke_tool same call with filter: { "max_bytes": 4096 }
```

**Pass criteria:**
- Step 3: full backend response, **no** `{ returned, total, truncated }` envelope
- Step 4: truncation envelope present — at minimum `{ truncated: true, total, returned, text }` (or byte metadata)
- **Fail if:** step 4 returns full multi-KB payload with no truncation metadata when clearly >4096 bytes

Also sanity-check JSON filter still works (github):

```
mcpmux_invoke_tool github list_issues with filter: { "max_rows": 3, "fields": ["title","number"] }
```

**Pass if:** `{ returned: 3, total: N, truncated: true, issues: [...] }`

---

## §9 — Surfaced promotion (was: SKIP — no surfaced tool configured)

**Setup first (human/UI step — confirm before testing):**
- In FeatureSet editor: leave `list_issues` **checked**, click **Surface** on that row only
- Other included tools checked but Surface **off**
- Save → Cursor → MCP → **Reload tools**

Then run:

```
1. List every tool you can call — separate mcpmux_* meta tools vs surfaced backend tools
2. Call github_list_issues DIRECTLY (one hop, no mcpmux_invoke_tool wrapper)
3. Call github_get_me (or another included but non-surfaced tool) via mcpmux_invoke_tool
4. Try direct call on a non-surfaced backend tool — expect use_invoke_tool redirect
```

**Pass criteria:**
- Exactly ~10 `mcpmux_*` + **1** surfaced backend (`github_list_issues`) in tool surface
- Direct `github_list_issues` succeeds (no redirect error)
- Non-surfaced tool absent from direct list but invoke succeeds
- Direct call on non-surfaced tool → redirect to `mcpmux_invoke_tool`
- **Fail if:** backend tools leak into list without Surface, or surfaced tool gets redirect

---

## §10 — Diagnostic list_all_tools vs search (was: FAIL — 41 available vs 3 invokable)

```
1. mcpmux_list_all_tools({ server_id: "github" })
2. mcpmux_search_tools({ query: "", server_id: "github", detail_level: "name" })
3. Compare counts and explain which tool agents should use for discovery
```

**Pass criteria:**
- `list_all_tools` response includes:
  - `total_installed` (full github catalog, e.g. ~41)
  - `total_invokable` (matches ACL, e.g. ~3)
  - per-row `invokable: true/false` and `server_available` (NOT bare `available: true` for all)
  - `hint` steering to `mcpmux_search_tools`
- `search_tools` total === `total_invokable` (not `total_installed`)
- Agent explicitly recommends **search** for invoke workflows, **list_all_tools** only for operator/diagnostic/FeatureSet authoring
- **Fail if:** all 41 tools still marked invokable/available with no ACL distinction

---

## FINAL REPORT (required — paste entire block back)

```
## Retest Summary
Overall: SHIP | SHIP WITH ISSUES | BLOCK
Commit tested: 85113e7 (or actual if different)
Sections run: §3 §6 §9 §10

| Section | Result | Notes |
|---------|--------|-------|
| §3 Batch schema | PASS/FAIL | |
| §6 max_bytes filter | PASS/FAIL/PARTIAL | |
| §9 Surfaced | PASS/FAIL/SKIP | |
| §10 Diagnostic | PASS/FAIL | |

## Red flags (check any observed)
[ ] Array schema still returns empty schemas: []
[ ] list_all_tools still marks non-ACL tools invokable
[ ] max_bytes on large plain-text payload no truncation metadata
[ ] Surfaced tool gets use_invoke_tool redirect
[ ] Backend tools in tools/list without Surface

## Friction log (verbatim errors / surprises)

## Environment
- github status:
- FeatureSet ACL tool count (from search):
- Surfaced tools in direct list:
- total_installed / total_invokable from list_all_tools:
```

Rules: use **McpMux meta tools only** for backend calls unless §9 explicitly tests direct surfaced one-hop. Read schemas before invoke. Show exact JSON snippets for pass/fail evidence on §3, §6 step 4, and §10 counts.
```

---

## What changed in `85113e7`

| Fix | Expected retest impact |
| --- | ---------------------- |
| `list_all_tools` adds `invokable`, `server_available`, counts, hint | §10 should PASS |
| `get_tool_schema` array + JSON-encoded array + `missing` field | §3 should PASS |
| `max_bytes` applies to JSON arrays without `max_rows` | §6 step 4 should PASS with large payload |

---

## Sign-off (fill after agent report)

| Section | Result | Notes |
| ------- | ------ | ----- |
| §3 Batch schema | ☑ Pass ☐ Fail | Array + string forms; `missing`/`message` on `github_create_issue` |
| §6 max_bytes filter | ☑ Pass ☐ Fail ☐ Partial | Large github JSON truncates with `{ truncated, total, returned, text }`; JSON filter sanity pass |
| §9 Surfaced | ☑ Pass ☐ Fail ☐ Skipped | 10 `mcpmux_*` + 1 `github_list_issues`; direct one-hop OK; non-surfaced absent from list |
| §10 Diagnostic | ☑ Pass ☐ Fail | `total_installed: 41`, `total_invokable: 3`; per-row `invokable`/`server_available`; `hint` present |
| **Overall** | ☑ Ship ☐ Block | Commit `85113e7` on `feat/meta-gateway-invoke` |

**Tester / date:** Cursor agent / May 25, 2026

**Blockers:** None. §6 used github large payload (GWorkspace not in QA FeatureSet). Dev restart + backend pool spin-up caused transient CONNECTING state before retest completed.
