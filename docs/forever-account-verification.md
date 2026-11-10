# Forever account verification

This change is for review. It does not enable a live feature.

## Endpoints and gates

- `BNET_FOREVER_LOOKUP` stays off by default. Public enrichment uses the existing
  realmless `/profile/wow/character/<first>-<surname>` helper. A public lookup is
  not ownership proof.
- `BNET_FOREVER_ACCOUNT` stays off by default. With explicit approval to enable
  it, OAuth sync makes a user-token GET to
  `https://us.api.blizzard.com/profile/user/wow?namespace=profile-forever-us&locale=en_US`.
  US host and `profile-forever-us` are assumptions for the **account** endpoint:
  the public character endpoint was tested, but the account endpoint has not yet
  been tested against a real linked account. No other region/namespace is guessed.
- `BNET_FOREVER_ACCOUNT_PROBE` is a separate temporary, default-off diagnostic.
  It also requires `BNET_FOREVER_ACCOUNT_PROBE_USER` to equal the intended
  Fraglink user UUID; absent/invalid/different targets do not probe. With ACCOUNT
  still off, it makes the same GET during that user's re-link and parses the
  result, logging parsed names/ids/levels and count (or HTTP status/schema failure),
  never tokens or raw response bodies. It never
  writes Forever cache/characters/hours. Ordinary Retail OAuth sync still runs;
  this is not a read-only mode for the entire existing OAuth callback.
- Tokens are never persisted or logged. The existing OAuth flow holds one in an
  in-memory stash for up to 15 minutes; the verification page consumes it once
  to fetch characters, then discards it. Re-link and complete that page for each
  probe/verification attempt. The probe is not a new public route.

Expected account schema: `wow_accounts[].characters[]` with positive `id`, separate
`name` and `surname`. `level`, `playable_class`, `faction.type` and literal `guid`
are optional. GUID availability in the account response is unconfirmed; no id is
inferred from the addon GUID. Missing required fields, duplicate names/ids, HTTP
errors (including 404), and partial/unknown schemas leave the Forever cache alone.
Only a successful, valid complete listing replaces that user's US Forever cache;
a valid empty listing revokes ownership, not hours. Retail cache is separate.

## Boundaries

Existing US realmless Forever rows match by full name, Blizzard id, or exact GUID
if supplied by both sources. Known conflicting ids/GUIDs are rejected. A match
must be unique in both directions and the upload BattleTag must equal the verified
linked BattleTag. A newer account claim elsewhere prevents verification.

Verification updates only `verified`, `ever_verified`, `bnet_id`, `bnet_level`,
`class` and `faction`. It does not rename (even on an id match), insert, merge or
remove character rows, change character UUIDs, /played, timestamps or addon GUIDs,
record/reduce/recount hours, or replay quarantine. Unlink only revokes Forever's
current tick; its hours/history remain. Retail reconciliation and recount logic
remain Retail-only. Generic realm-based level/spec/id backfills exclude Forever.

Migration 0062 adds nullable GUID/faction fields to the account cache only. No
backfill or data rewrite is needed. The previous binary can ignore these columns.

## Release and real-account test plan

1. Wait for Lucrona's explicit **release** and green review/CI. No merge/deploy or
   live flag/database changes are authorised by this PR.
2. Take `/run/lock/fraglink-deploy.lock`, back up under `/var/backups/fraglink/`,
   verify the backup, then follow the private deploy runbook and `scripts/deploy.sh`
   (cargo zigbuild). Deploy with LOOKUP and ACCOUNT both **off**. Capture pre/post
   character UUIDs, seconds, hour totals, history and ledger fingerprints.
3. With approval for the diagnostic, turn on only ACCOUNT_PROBE and set
   ACCOUNT_PROBE_USER to Lucrona's Fraglink UUID. Lucrona re-links
   Battle.net; the Forever GET/parser remains read-only. Check that the returned
   complete listing contains the expected full names and stable ids for Lucrona
   Ashborne (1119743) and Lucroner Ashborne (1395710). Do not retain token/body dumps.
   Compare the limited structured probe output, not just its count. Keep these
   account metadata logs private; no raw payload or token logging is needed.
4. Turn the probe off and clear its target UUID. If the endpoint, schema or ownership list is wrong or
   incomplete, leave ACCOUNT off and adjust the PR. Do not fall back to public
   profiles as proof. Do not replay uploads or use real accounts for synthetic QA.
5. Report the read-only result and get Lucrona's explicit OK before turning on
   `BNET_FOREVER_ACCOUNT`. LOOKUP can remain off. She then re-links **again**,
   because the probe consumed the one-time in-memory token. Check the same existing character
   UUIDs now carry the tick and allowed metadata only. Compare seconds, totals,
   snapshots, histories, quarantine and ledger fingerprints exactly.
6. Stop and report any mismatch; do not repair hours by rewriting them. Turning
   ACCOUNT off stops future account checks; no rollback rewrites/deletions are part
   of this plan. Do not build the desktop app.
