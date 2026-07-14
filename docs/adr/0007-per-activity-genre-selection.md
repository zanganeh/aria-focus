# ADR 0007: Per-activity genre selection

Genre preference is an optional stable taxonomy ID stored independently for each activity. `NULL`/no row means **Any compatible genre**. The catalogue receives that ID as a pure selection input and requires an exact `item.genre_ids` match for every selected track, including a crossfade partner.

The backend revalidates every installed pack before listing options and before playback. It offers only labels whose approved items are playable and suitable for the active activity, in stable ID order. A saved ID not in that set is returned as unavailable; the user must explicitly choose Any or an available ID. It is never silently substituted.

With Any selected, no eligible installed content may use the existing explicit procedural test-tone fallback. With a concrete genre, no matching selection returns an actionable error and never claims the test tone meets that genre. Preferences use migration 0004 and reject malformed identifier values on both write and read. The backend owns the genre preference through the pack service's SQLite repository; commands acquire the pack mutex before the core mutex to avoid inversion with playback commands.

Limitations: taxonomy labels are supplied by content packs. When multiple installed packs use a single ID with different labels, the lexically smallest label is exposed deterministically; content authors should keep shared IDs consistently labelled.
