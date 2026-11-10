# Forever names (1.1.1)

The 1.1.0 addon read only the first result of `UnitName("player")`, then tried to
split that string. Forever returns the surname as the **second** result. This
explains first-name-only files even though the character has a surname.

Sources checked on 10 November 2026:
- https://github.com/ATTWoWAddon/AllTheThings/issues/2630 — reports observed
  `UnitName`, `UnitNameUnmodified` and `UnitFullName` first/surname tuples, and
  `GetUnitName` returning the complete name.
- https://github.com/ATTWoWAddon/AllTheThings/pull/2633 — merged upstream fix.
- https://github.com/obelisk2u/ForeverTracker/blob/main/docs/RESEARCH_FOREVER_API.md
  — corroborates the name tuples and documents player GUID collection. Its
  pre-launch project-ID observations are outdated relative to Fraglink's tested
  project-18 client and are not used to change client detection.

The addon now takes Forever's `UnitName` surname, falls back to `UnitFullName`'s
second result, then the matching first-name prefix of `GetUnitName`'s full name.
It does not read selected titles or interpret a Retail realm as a surname.
`UnitGUID("player")` is stored as an opaque string, not parsed into an identity.
The v3 checksum covers the GUID; v1/v2 checksums remain accepted by the server.

Server identity is scoped to the owner, Forever, region and realmless namespace.
A signed GUID identifies the character regardless of display surname. Older
GUID-less files can enrich one compatible first-name match; multiple possible
matches go to review, never an automatic merge. Different known GUIDs are never
combined. Missing surnames cannot erase a known surname. No database backfill,
quarantine replay or existing character/hour merge is part of this change.

These API contracts are confirmed by community reports and a merged upstream
fix, but were not exercised in the live 70862 client during this work. Lua mocks
cover the real two-return shape, both fallbacks, Retail separation and safe
legacy-slot upgrades. A live 1.1.1 login/export check remains release QA.
