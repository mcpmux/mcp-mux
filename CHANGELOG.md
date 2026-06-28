# Changelog

## [0.6.0](https://github.com/mcpmux/mcp-mux/compare/v0.5.0...v0.6.0) (2026-06-28)


### Features

* **gateway:** optional network access — bind 0.0.0.0 for LAN sharing ([#200](https://github.com/mcpmux/mcp-mux/issues/200)) ([9e481e7](https://github.com/mcpmux/mcp-mux/commit/9e481e71b5857d9f67b62a811694034bed3a4400))
* support configurable public gateway base URL ([#192](https://github.com/mcpmux/mcp-mux/issues/192)) ([6f81378](https://github.com/mcpmux/mcp-mux/commit/6f81378384ed37cf254d7b8711199404a848ca3f))


### Bug Fixes

* avoid synthetic connecting state for enabled servers ([#196](https://github.com/mcpmux/mcp-mux/issues/196)) ([80045c6](https://github.com/mcpmux/mcp-mux/commit/80045c61e9ea2e92a035ab59f6d0adbad33175e0))
* filter invalid DCR redirect URIs ([#193](https://github.com/mcpmux/mcp-mux/issues/193)) ([187b57c](https://github.com/mcpmux/mcp-mux/commit/187b57c31f68519dd536ee8e576cbae0b9a2cf08))
* make feature discovery capability-aware and bounded ([#194](https://github.com/mcpmux/mcp-mux/issues/194)) ([c2a1569](https://github.com/mcpmux/mcp-mux/commit/c2a156974004e113b4368dada41411e61c8205f1))
* restore titlebar drag region without breaking controls ([#197](https://github.com/mcpmux/mcp-mux/issues/197)) ([47c787c](https://github.com/mcpmux/mcp-mux/commit/47c787cf95b1c36e99673596170666252c59f114))
* sync custom server config saves immediately ([#195](https://github.com/mcpmux/mcp-mux/issues/195)) ([536a4ce](https://github.com/mcpmux/mcp-mux/commit/536a4ceb7e4ad32549f870145557d00fe790cb2e))

## [0.5.0](https://github.com/mcpmux/mcp-mux/compare/v0.4.0...v0.5.0) (2026-06-25)


### Features

* per-workspace routing via X-Mcpmux-Workspace header + guided folder setup ([#182](https://github.com/mcpmux/mcp-mux/issues/182)) ([e2ec055](https://github.com/mcpmux/mcp-mux/commit/e2ec0558eada73407addc57902d9f13763cc8aec))
* **ui:** opencode global connect + client icons ([#184](https://github.com/mcpmux/mcp-mux/issues/184)) ([669e99f](https://github.com/mcpmux/mcp-mux/commit/669e99f2df3d85bbdd33427f0a96bd8e0048c7c3))


### Bug Fixes

* **gateway:** truly no-auth when inbound auth is disabled (no OAuth advertising) ([#187](https://github.com/mcpmux/mcp-mux/issues/187)) ([3e617fd](https://github.com/mcpmux/mcp-mux/commit/3e617fd876360f097fbffa96e21ce1fb90013fea))
* **oauth:** de-duplicate deep-link handling + quiet status-poll log ([#189](https://github.com/mcpmux/mcp-mux/issues/189)) ([9dd7b58](https://github.com/mcpmux/mcp-mux/commit/9dd7b58a23cef1f7f6a5ce53cab20750270c1a26))
* **storage:** drop a deleted FeatureSet from workspace bindings ([#186](https://github.com/mcpmux/mcp-mux/issues/186)) ([5598451](https://github.com/mcpmux/mcp-mux/commit/559845193aae3bc5bffff1e41758ffce1c083625))
* **ui:** scroll-to + flash the target Settings section on every redirect ([#190](https://github.com/mcpmux/mcp-mux/issues/190)) ([e032c9b](https://github.com/mcpmux/mcp-mux/commit/e032c9bc2e5a99ea64df4a4af215c5a29153af52))
* **ui:** show official opencode logo in the Apps tab ([#185](https://github.com/mcpmux/mcp-mux/issues/185)) ([608a841](https://github.com/mcpmux/mcp-mux/commit/608a841b1b0d058e055ff2ff0ba6cc37ed921a33))

## [0.4.0](https://github.com/mcpmux/mcp-mux/compare/v0.3.0...v0.4.0) (2026-06-19)


### Features

* [@mux](https://github.com/mux) UX + Windows updater fix + minimal-first optimization ([#171](https://github.com/mcpmux/mcp-mux/issues/171)) ([a215012](https://github.com/mcpmux/mcp-mux/commit/a215012ccd37388ffc6d802452e2fe03c9ce1ea5))
* **featureset:** protect Starter from deletion + clarify mapping popup ([#176](https://github.com/mcpmux/mcp-mux/issues/176)) ([163ee0b](https://github.com/mcpmux/mcp-mux/commit/163ee0b0ef0ac0166be0ecf9e2f8bad1612dfad3))
* **gateway:** default FeatureSet for unmapped roots + Mapped workspaces filter ([#175](https://github.com/mcpmux/mcp-mux/issues/175)) ([7fc50a0](https://github.com/mcpmux/mcp-mux/commit/7fc50a00923f74f752e5c279f60e232c1865c3e5))
* pre-release update channel + automated pre-releases from main ([#159](https://github.com/mcpmux/mcp-mux/issues/159)) ([e9306c4](https://github.com/mcpmux/mcp-mux/commit/e9306c4a8ac1aee72be2530697a643b69fb130f6))
* **spaces:** per-space base directories scope workspace roots to a Space ([#179](https://github.com/mcpmux/mcp-mux/issues/179)) ([fb825cf](https://github.com/mcpmux/mcp-mux/commit/fb825cfe66c6f383ea1f52d290a5519a67bffd5f))
* workspace-root routing + Tool Optimization ([@mux](https://github.com/mux)) self-management + UI live-sync ([#151](https://github.com/mcpmux/mcp-mux/issues/151)) ([d614853](https://github.com/mcpmux/mcp-mux/commit/d6148538b6f40644f9367d3c872bc1f4f2f7be63))
* **workspaces:** bulk-clear unmapped folders + clearer approval opt-out ([#172](https://github.com/mcpmux/mcp-mux/issues/172)) ([09b561c](https://github.com/mcpmux/mcp-mux/commit/09b561c901170cac2c6546ef8680b313f292eee6))
* **workspaces:** setting to disable the new-folder mapping prompt ([#177](https://github.com/mcpmux/mcp-mux/issues/177)) ([d5df002](https://github.com/mcpmux/mcp-mux/commit/d5df002df03e9b80a0bd780ce9c52fd9e942d02e))


### Bug Fixes

* add Windsurf, JetBrains, and Android Studio to quick-connect grid ([#139](https://github.com/mcpmux/mcp-mux/issues/139)) ([fb58d9c](https://github.com/mcpmux/mcp-mux/commit/fb58d9ce6c46ec1a55356a9fecb35f34ae2b29f6))
* **deps:** resolve 4 transitive security advisories failing Dependabot ([#174](https://github.com/mcpmux/mcp-mux/issues/174)) ([eb32289](https://github.com/mcpmux/mcp-mux/commit/eb32289ca7f309c52223f17cf3a1e1c0f0a61d7c))
* **gateway:** ride out self-update port race + clearer update restart UX ([#173](https://github.com/mcpmux/mcp-mux/issues/173)) ([6868992](https://github.com/mcpmux/mcp-mux/commit/6868992faeb77c8fe32ad7c996b2d196ca586002))
* **oauth:** DCR skip-invalid redirect URIs + drop duplicate RFC 8707 resource param ([#158](https://github.com/mcpmux/mcp-mux/issues/158)) ([661f162](https://github.com/mcpmux/mcp-mux/commit/661f1620105803acfe07087e997a1d4d00aa77d5))
* **servers:** pin config-modal footer ([#163](https://github.com/mcpmux/mcp-mux/issues/163)) + silent Windows updates ([#165](https://github.com/mcpmux/mcp-mux/issues/165)) ([0ddbdb5](https://github.com/mcpmux/mcp-mux/commit/0ddbdb59c7229f8bd9dd0a4875216af5ab8977af))
* **spaces:** clearer base-directories UX ([#180](https://github.com/mcpmux/mcp-mux/issues/180)) ([4a69908](https://github.com/mcpmux/mcp-mux/commit/4a699085087b775439180ace2467cda708b26fc3))
* **storage:** purge orphaned feature_set_members after the refactor (migration 017) ([#167](https://github.com/mcpmux/mcp-mux/issues/167)) ([b90b05c](https://github.com/mcpmux/mcp-mux/commit/b90b05c038d9d1bc8fae395e83e0e8db713d3e3f))


### Documentation

* add Discord community link to README ([#149](https://github.com/mcpmux/mcp-mux/issues/149)) ([c32f78f](https://github.com/mcpmux/mcp-mux/commit/c32f78f7143177589ea96b4e33170f49cc343b30))
* add Workspaces and Tool Optimization guides ([#164](https://github.com/mcpmux/mcp-mux/issues/164)) ([25a6fbb](https://github.com/mcpmux/mcp-mux/commit/25a6fbb94d3efcbd0cd5a714ac452c5e61608250))
* complete the getting-started flow + workspace-driven routing ([#166](https://github.com/mcpmux/mcp-mux/issues/166)) ([92f8ac2](https://github.com/mcpmux/mcp-mux/commit/92f8ac2f053e27b4a1aec222eea7ff3f9986559c))

## [0.3.0](https://github.com/mcpmux/mcp-mux/compare/v0.2.3...v0.3.0) (2026-02-25)


### Features

* post-action UX guidance, ConfirmDialog, and client auto-select ([#136](https://github.com/mcpmux/mcp-mux/issues/136)) ([44d934c](https://github.com/mcpmux/mcp-mux/commit/44d934c678c4d7a2eebc996928e2fb37c07d7a8e))

## [0.2.3](https://github.com/mcpmux/mcp-mux/compare/v0.2.2...v0.2.3) (2026-02-21)


### Bug Fixes

* allow process restart after update and detect Homebrew version mismatch ([#134](https://github.com/mcpmux/mcp-mux/issues/134)) ([ecdbaca](https://github.com/mcpmux/mcp-mux/commit/ecdbacafaff573f497ce6db8614fa39993a28a32))
* debounce analytics search tracking to capture final query ([#132](https://github.com/mcpmux/mcp-mux/issues/132)) ([0f17ddb](https://github.com/mcpmux/mcp-mux/commit/0f17ddb768b5d309a3a73cc6df492f656e205f69))

## [0.2.2](https://github.com/mcpmux/mcp-mux/compare/v0.2.1...v0.2.2) (2026-02-20)


### Bug Fixes

* detect OAuth requirement from unexpected content-type responses ([#128](https://github.com/mcpmux/mcp-mux/issues/128)) ([d894d17](https://github.com/mcpmux/mcp-mux/commit/d894d17c7c4c5841b7eb39dc1d7068dbcb447656))
* wire up HTTP definition headers orthogonally from auth ([#125](https://github.com/mcpmux/mcp-mux/issues/125)) ([04380e0](https://github.com/mcpmux/mcp-mux/commit/04380e0979ab428351185d381001d209e6a4993b))


### Documentation

* add user guide with screenshots ([#130](https://github.com/mcpmux/mcp-mux/issues/130)) ([a97a133](https://github.com/mcpmux/mcp-mux/commit/a97a1333520fc1ac54f061344970cf493807ca87))
* add user guide with screenshots ([#131](https://github.com/mcpmux/mcp-mux/issues/131)) ([ee28e8b](https://github.com/mcpmux/mcp-mux/commit/ee28e8be432d2b1532f3f98067ba9004c4a18374))

## [0.2.1](https://github.com/mcpmux/mcp-mux/compare/v0.2.0...v0.2.1) (2026-02-19)


### Bug Fixes

* regenerate ICO with proper sizes & increase connection timeout ([#123](https://github.com/mcpmux/mcp-mux/issues/123)) ([2d88b25](https://github.com/mcpmux/mcp-mux/commit/2d88b259e9ca1bbc1ac57405854d732d8437cce3))


### Refactoring

* remove Password and Textarea from InputType enum ([#122](https://github.com/mcpmux/mcp-mux/issues/122)) ([bd06386](https://github.com/mcpmux/mcp-mux/commit/bd06386e04020da381135761a631ab38543ae414))

## [0.2.0](https://github.com/mcpmux/mcp-mux/compare/v0.1.2...v0.2.0) (2026-02-18)


### Features

* add select, file_path, and directory_path input types ([#121](https://github.com/mcpmux/mcp-mux/issues/121)) ([942ee1a](https://github.com/mcpmux/mcp-mux/commit/942ee1ae88f60aa1454bc97cec3839bcacf74454))


### Bug Fixes

* add one-click IDE install for VS Code and Cursor ([#119](https://github.com/mcpmux/mcp-mux/issues/119)) ([5b280fb](https://github.com/mcpmux/mcp-mux/commit/5b280fbfdcd04165827b7662ba6896cea96deb83))
* version display & update check ([#117](https://github.com/mcpmux/mcp-mux/issues/117)) ([b40c59b](https://github.com/mcpmux/mcp-mux/commit/b40c59bfb7b9ec19be8848abe04e38ba6fed1422))

## [0.1.2](https://github.com/mcpmux/mcp-mux/compare/v0.1.1...v0.1.2) (2026-02-18)


### Bug Fixes

* resolve npx/node PATH on macOS GUI apps ([#113](https://github.com/mcpmux/mcp-mux/issues/113)) ([98c013d](https://github.com/mcpmux/mcp-mux/commit/98c013d4e6955e678949df6068c038e1b8cf00fc))


### Documentation

* improve README first impression with problem/fix diagrams ([#109](https://github.com/mcpmux/mcp-mux/issues/109)) ([b15482b](https://github.com/mcpmux/mcp-mux/commit/b15482b32a016e3ca92753f26212f5827f744903))

## [0.1.1](https://github.com/mcpmux/mcp-mux/compare/v0.1.0...v0.1.1) (2026-02-16)


### Bug Fixes

* file-based keychain fallback for headless Linux/WSL ([#103](https://github.com/mcpmux/mcp-mux/issues/103)) ([9b60e0b](https://github.com/mcpmux/mcp-mux/commit/9b60e0bbe47a2318e7352efd3ba8b1888f393f38))
* stdio enable error UI state ([#104](https://github.com/mcpmux/mcp-mux/issues/104)) ([b4598e6](https://github.com/mcpmux/mcp-mux/commit/b4598e60e12d3389717fc2252bac8eb29e96f9c9))

## [0.1.0](https://github.com/mcpmux/mcp-mux/compare/v0.0.1...v0.1.0) (2026-02-16)

First public release of McpMux — the unified MCP gateway and manager for AI clients.

### Features

* Unified MCP gateway — configure servers once, connect every AI client through a single endpoint
* Encrypted credential storage via OS keychain (DPAPI, Keychain, Secret Service) with AES-256-GCM field-level encryption
* Spaces for organizing servers into workspaces with per-client access key authentication
* FeatureSet filtering — fine-grained control over tools, resources, and prompts per client
* OAuth 2.1 + PKCE with automatic token refresh for OAuth-enabled MCP servers
* Server discovery — browse and install from the community registry at mcpmux.com
* Streamable HTTP transport with SSE notifications
* Stdio transport with platform-specific process isolation
* Server connection logging with MCP protocol notifications and stderr capture
* Custom server configuration fields — environment variables, arguments, and headers
* Default values for server input definitions
* McpMux-branded OAuth authorization pages
* System tray with autostart on login
* Built-in auto-updater with signed releases
* Cross-platform installers — Windows (NSIS), macOS (DMG via Homebrew), Linux (APT + AppImage + .deb)
