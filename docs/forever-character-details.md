# Forever character details and share links

Read-only public API probe, 10 November 2026 (AEDT), using app credentials only.
No account tokens, database writes or live switches were involved. The two tested
Forever profiles were Lucrona Ashborne and Lucroner Ashborne on the US host with
`profile-forever-us`. Both returned the same resource shapes.

| Resource | Result | Useful fields |
| --- | --- | --- |
| Character | 200 | id, name + surname, level, race/class/faction, item levels, points |
| equipment | 200 | equipped_items: slots, item/media ids, name, quality, level, armor/stats, durability |
| statistics | 200 | health, resource, attributes, armor/defense, attack power, crit/dodge/parry/block and more |
| character-media | 200 | main-raw/main-sd-raw PNG renders, avatar/avatar-sd JPEGs |
| specializations | 200 | active specialization_groups, spent_points, talents |
| appearance | 200 | items, customizations, race/class/gender/faction, optional guild_crest |
| professions | 200 | primaries/secondaries, tiers, skill/max, recipes |
| achievements | 200 | total_quantity/points, achievement entries, statistics link |
| achievements/statistics | 200 | nested categories |
| pvp-summary | 200 | honorable_kills, pvp_rank (not Retail honor level/rated brackets) |
| encounters/raids | 200 | character link only; no raid progress for these characters |
| guild subresource | 404 | neither root profile had a guild |
| static-forever-us item media | 200 for all 25 distinct equipped item ids | icon assets, including numeric file-data ids |

The existing icon proxy's US CDN path was checked with a Forever numeric icon and
returned 200 too. The official Forever Armory route was subsequently verified with both characters:
`https://worldofwarcraft.blizzard.com/en-us/forever/us/armory/character/<first>-<surname>`.
Both pages returned HTTP 200 with the correct character title and Blizzard id in
the HTML (Lucrona Ashborne / 1119743 and Lucroner Ashborne / 1395710). No redirect
was needed. Forever uses that realmless route, not Retail's realm-based path.
There is no Retail Wowhead item lookup in Forever popups.

## Popup

The existing popup layout now accepts Forever when `BNET_FOREVER_LOOKUP` is on
(default remains off). This public lookup is not ownership verification and never
adds a tick. It can show unverified Forever rows already shown on the profile.
Retail still requires verification. All existing profile privacy, hidden-character,
hidden-game, hide-all and Show on profile rules apply on the server too.

Forever uses the realmless URL, checks the returned full name and any known
Blizzard id, and caches only its summarised details for 12 hours with the existing
per-character lock/stale-refresh behaviour. Item-media memory caching is version-
scoped, separate from Retail's item-id cache. No character/hour/identity writes
are part of this refresh. Failed optional resources quietly disappear; a failed
first lookup closes the popup. Last good cached summaries are retained.

Displayed data: name/level/race/class, dominant active talent specialization,
actual equipped slots (including ranged), item icons/quality/level, character
render or avatar, core combat stats, professions and actual PvP activity. Empty
gear slots and absent sections are omitted in both versions. Neither raw recipes,
all achievements nor appearance customizations are fetched just to fill a box.
The full achievements payload is intentionally not fetched by the popup.

## Stable links and previews

`/u/<handle>/wow/retail/<Fraglink character UUID>`
`/u/<handle>/wow/forever/<Fraglink character UUID>`

Opening pushes a share URL and offers Share (copies the link, then briefly says Link copied); closing returns to the previous
profile/gallery URL. A direct visit opens the profile with the popup; closing a
direct visit stays on the profile rather than navigating away. Browser back and
forward are supported. The character name is not part of the stable identifier.

Automatic opening first checks a public-only metadata endpoint. Private, hidden,
wrong-version or unknown characters leave the normal profile open without a
character error. This public-only check applies even when the visitor is the
owner. Normal owner/friend popup access outside a shared entry retains the existing
visibility model.

The HTML route uses the same anonymous visibility check before emitting character
Open Graph title, level/class/guild description and cached render image (generic
site image when no render is cached). It does not fetch Blizzard for a crawler.
Guild text is omitted unless Show guild is on. Share pages remain noindex. Hidden,
private and unknown character ids all get generic profile tags, with no character
name or image in the HTML. Images re-check visibility and are served no-store.

No new migration for these popup/share additions; migration 0062 from the first
part of PR 35 remains the only migration. Nothing is enabled or released here.

The footer keeps Share and View on Armory together on one line for both versions.
