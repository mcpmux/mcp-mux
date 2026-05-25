# Meta-Gateway Invoke — Manual QA Runbook

**Last Updated:** May 25, 2026  
**Branch:** `feat/meta-gateway-invoke`  
**Related:** [`meta-gateway-invoke.md`](./meta-gateway-invoke.md)

One-session checklist for validating Phases A–C (search → schema → invoke, result shaping, FeatureSet ACL + surfaced tools).

---

## Quick prep

- [ ] Rebuild/restart gateway if you haven't since the branch (`pnpm dev` or run the built app)
- [ ] Cursor → MCP → **Reload tools**
- [ ] Confirm McpMux endpoint: `http://localhost:45818/mcp`
- [ ] Have at least one OAuth server (GitHub) **installed and connected** but **inactive** in session (for enable-flow tests)
- [ ] Optional for Phase C tests: create a FeatureSet with 1–2 GitHub tools, bind to workspace; leave surfaced off until test 7

**Tester:** _______________  
**Date:** _______________  
**McpMux version / commit:** _______________

---

## 0. Sanity — meta-only surface

**Prompt:**

```
You have McpMux meta tools only — no direct backend tools like github_*.

1. Call mcpmux_list_servers and show installed servers and active/inactive status.
2. Tell me how many tools you see in your available tool list total, and list their names.
```

| Check | Pass | Fail | Notes |
| ----- | ---- | ---- | ----- |
| `mcpmux_list_servers` returns installed servers | ☐ | ☐ | |
| Only **10** `mcpmux_*` tools exposed (no backend names) | ☐ | ☐ | Expected: bind, create_feature_set, disable/enable_server, get_tool_schema, invoke_tool, list_all_tools, list_feature_sets, list_servers, search_tools |
| Backend servers show **inactive** until enabled | ☐ | ☐ | |
| Tool list count stable (~10 meta + Cursor/plugin tools) | ☐ | ☐ | |

---

## 1. Happy path — GitHub read (Phase A)

**Prompt** (swap repo if needed):

```
Use ONLY the McpMux meta workflow — do not guess backend tool names or params.

Goal: list open issues in mcpmux/mcp-mux.

Steps you must follow explicitly:
1. mcpmux_list_servers — check if github is active
2. If inactive: mcpmux_enable_server for github
3. mcpmux_search_tools with query "list issues", server_id "github", detail_level "description"
4. mcpmux_get_tool_schema for the best match
5. mcpmux_invoke_tool with exact args from the schema

Show each step briefly, then the first 5 issues.
```

| Check | Pass | Fail | Notes |
| ----- | ---- | ---- | ----- |
| Agent enabled github when inactive | ☐ | ☐ | |
| Search before invoke (no param guessing) | ☐ | ☐ | |
| Schema read before invoke | ☐ | ☐ | |
| Invoke succeeded with correct param names | ☐ | ☐ | |
| `tools/list` still ~10 meta tools after enable | ☐ | ☐ | |

---

## 2. Fail-closed + recovery (Phase A errors)

**Prompt:**

```
Try to invoke a GitHub tool WITHOUT enabling github first (disable it if needed).

1. mcpmux_invoke_tool on github with tool list_issues and dummy args
2. Show the exact error message
3. Follow whatever it tells you to do
4. Retry invoke successfully
```

| Check | Pass | Fail | Notes |
| ----- | ---- | ---- | ----- |
| Invoke denied when server inactive | ☐ | ☐ | |
| Error mentions `mcpmux_enable_server` with server_id | ☐ | ☐ | |
| Recovery via enable → retry works | ☐ | ☐ | |

---

## 3. Search detail levels + compact schema (Phase A)

**Prompt:**

```
On github (enabled):

1. mcpmux_search_tools query "list" detail_level "name" limit 5
2. Same query detail_level "description"
3. Pick one tool — mcpmux_get_tool_schema compact: true
4. Same tool — compact: false

What did compact strip?
```

| Check | Pass | Fail | Notes |
| ----- | ---- | ---- | ----- |
| `name` level omits descriptions | ☐ | ☐ | |
| `description` level includes descriptions | ☐ | ☐ | |
| `compact: true` strips descriptions/examples | ☐ | ☐ | |
| Batch schema (array of tools) works if agent tries it | ☐ | ☐ | |

---

## 4. Session toggle — list size unchanged (Phase A)

**Prompt:**

```
1. Enable github — confirm search finds github tools
2. Disable github via mcpmux_disable_server
3. Search again for github tools
4. Report tools/list count before and after — must stay the same
```

| Check | Pass | Fail | Notes |
| ----- | ---- | ---- | ----- |
| Search empty / no github matches when disabled | ☐ | ☐ | |
| Meta tool count unchanged across enable/disable | ☐ | ☐ | |

---

## 5. Default truncation (Phase B)

**Setup:** Enable a heavy server — `posthog-personal`, `firebase-dev`, or GWorkspace clone.

**Prompt:**

```
Enable [heavy server]. Find a list/analytics tool via search, read schema, invoke WITHOUT filter.

Show whether response includes { returned, total, truncated: true } or similar metadata.
Paste payload size estimate (rough char count is fine).
```

| Check | Pass | Fail | Notes |
| ----- | ---- | ---- | ----- |
| Large array auto-truncated | ☐ | ☐ | Default ~50 rows / 64KB |
| Truncation metadata present | ☐ | ☐ | |

---

## 6. Explicit filter (Phase B)

**Prompt:**

```
Same tool as test 5. Invoke with filter: { "max_rows": 3, "format": "summary" }

Then again with fields projection if the tool returns objects with id/name/title fields.
```

| Check | Pass | Fail | Notes |
| ----- | ---- | ---- | ----- |
| `max_rows: 3` honored | ☐ | ☐ | |
| `format: summary` applied | ☐ | ☐ | |
| `fields` projection limits keys per row (if tested) | ☐ | ☐ | |

---

## 7. Clone disambiguation (server_id filter)

**Setup:** You have GWorkspace ×2 clones — enable **only one**.

**Prompt:**

```
Enable ONLY taylorwilsdon.google-workspace-mcp-uvx (not the s2h clone).

mcpmux_search_tools query "drive" or "list files" with server_id set explicitly.
Confirm results are scoped to that server_id only.
```

| Check | Pass | Fail | Notes |
| ----- | ---- | ---- | ----- |
| `server_id` filter scopes search | ☐ | ☐ | |
| Other clone's tools not in results | ☐ | ☐ | |

---

## 8. FeatureSet ACL — partial tool set (Phase C)

**Setup:** FeatureSet with 1–2 GitHub tools included, bound to workspace, surfaced **off**.

**Prompt:**

```
I bound a FeatureSet that only allows specific GitHub tools.

1. mcpmux_search_tools query "github" detail_level "name"
2. Try mcpmux_invoke_tool on a tool NOT in the FeatureSet
3. Invoke one tool that IS included
```

| Check | Pass | Fail | Notes |
| ----- | ---- | ---- | ----- |
| Search only finds allowed tools | ☐ | ☐ | |
| Invoke denied for disallowed tool | ☐ | ☐ | |
| Invoke succeeds for allowed tool | ☐ | ☐ | |

---

## 9. Surfaced tool promotion (Phase C)

**Setup:** In FeatureSet editor, toggle **Surface in client** on one included tool. Reload MCP tools.

**Prompt:**

```
1. List all tools available — identify mcpmux_* vs surfaced backend
2. Call the surfaced tool directly (one hop)
3. Call a different tool on same server via mcpmux_invoke_tool
```

| Check | Pass | Fail | Notes |
| ----- | ---- | ---- | ----- |
| Surfaced tool appears in client tool list | ☐ | ☐ | |
| Surfaced tool callable without invoke wrapper | ☐ | ☐ | |
| Non-surfaced backend still requires invoke | ☐ | ☐ | |

---

## 10. Diagnostic — list_all_tools vs search

**Prompt:**

```
mcpmux_list_all_tools with server_id "github" (or one enabled server).
Compare count to mcpmux_search_tools with query "" and same server_id.
Explain why agents should prefer search.
```

| Check | Pass | Fail | Notes |
| ----- | ---- | ---- | ----- |
| `server_id` filter on list_all_tools works | ☐ | ☐ | |
| Agent recommends search over full dump | ☐ | ☐ | |

---

## 11. End-to-end agent task (realism)

**Prompt:**

```
Brief status report on mcpmux/mcp-mux repo:
- open issue count
- 3 most recent issue titles
- one paragraph summary

Rules: McpMux meta tools only, read schemas before invoke, note truncation if any.
```

| Check | Pass | Fail | Notes |
| ----- | ---- | ---- | ----- |
| Completed without backend tool name guessing | ☐ | ☐ | |
| Schema-first invoke pattern | ☐ | ☐ | |
| Sensible output despite truncation | ☐ | ☐ | |

---

## Red flags (stop and file a bug)

- [ ] Backend tools (`github_*`, etc.) appear in `tools/list` without surfacing
- [ ] Agent can call backend tools directly (bypassing `mcpmux_invoke_tool`)
- [ ] Enable server expands `tools/list` beyond meta + surfaced
- [ ] Search returns tools from inactive or unbound servers
- [ ] Invoke succeeds for tools outside FeatureSet ACL
- [ ] Large list invoke returns unbounded payload with no truncation metadata
- [ ] Opaque errors (no enable/invoke redirect hints)

---

## Sign-off

| Area | Result |
| ---- | ------ |
| Phase A — meta invoke core | ☐ Pass ☐ Fail |
| Phase B — result shaping | ☐ Pass ☐ Fail |
| Phase C — ACL + surfaced | ☐ Pass ☐ Fail ☐ Skipped |
| Overall | ☐ Ship ☐ Block |

**Blockers / issues filed:**

```
```
