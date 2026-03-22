# TMDB Search Matching Logic

> **USING THE API**

Analysis of how `search/tv` matches queries against TV show titles.
Based on TMDB official documentation and Travis Bell (TMDB maintainer) statements.

## Search Target Fields and Priority

The `search/tv` endpoint matches the query against **three fields**, scored by Elasticsearch with the following priority:

| Priority    | Field               | Description                                   | Example (SPY×FAMILY)                     |
| ----------- | ------------------- | --------------------------------------------- | ---------------------------------------- |
| 1 (highest) | `original_name`     | Original title (one per entry)                | `SPY×FAMILY`                             |
| 2           | `name` (translated) | Translated name based on `language` parameter | `SPY×FAMILY` (ja-JP)                     |
| 3 (lowest)  | alternative titles  | User-contributed titles per country/region    | `スパイファミリー`, `Spy x Family`, etc. |

All three fields are **always searched regardless of the `language` parameter**.

## Elasticsearch Scoring

- `original_name` match receives the highest score boost
- Exact matches are boosted over partial matches
- `popularity` is added as a score factor (popular titles rank higher for ambiguous queries)
- The API uses ngram-based tokenization for partial matching

## `language` Parameter Behavior

The `language` parameter controls **response language only**:

- Sets which translated `name` appears in the response
- Does **not** filter which fields are searched
- Searching with `language=ja-JP` still matches against English `original_name` and all alternative titles

## Alternative Titles (alternative_titles)

Alternative titles are user-contributed per country/region. Their quality and completeness varies.

### Arc/Season Variants in Alternative Titles

Alternative titles frequently include arc names and season indicators:

**Demon Slayer (TMDB ID: 85937)** — JP alternative titles:

- `鬼滅の刃 竈門炭治郎 立志編`
- `鬼滅の刃 無限列車編`
- `鬼滅の刃 遊郭編`
- `鬼滅の刃 刀鍛冶の里編`
- `鬼滅の刃 柱稽古編`

**SPY×FAMILY (TMDB ID: 120089)** — alternative titles:

- `SPY×FAMILY：2022`, `Spy X Family`, `Spy x Family 2nd`, `スパイファミリー`
- HK/TW variants: `SPY FAMILY 間諜家家酒 Season 2`, etc.

### Implications

- Arc-named titles from Syoboi Calendar (e.g., `鬼滅の刃 柱稽古編`) may match alternative titles, but searching with the base title `鬼滅の刃` hits `original_name` (priority 1) for a higher score
- Season indicators like `第N期` are **not** registered in `original_name` or alternative titles — they must be removed before search
- Quality is uneven: some shows have comprehensive alternatives, others have none

## Recommended Search Strategy for Anime Titles

1. **Base title search** — Remove season indicators, then search with the base title to match `original_name` (highest priority)
2. **Year filter** — Use `first_air_date_year` to disambiguate remakes and same-name shows
3. **Decoration fallback** — If no results, remove decorative characters (`☆`, `×`, etc.) and retry
4. **Result validation** — Compare `original_name` from results against the normalized query using similarity scoring

## Season Indicator and Search Behavior

| Pattern                      | In `original_name`?     | In alternative titles? | Recommendation                                      |
| ---------------------------- | ----------------------- | ---------------------- | --------------------------------------------------- |
| `第N期` / `Season N`         | No                      | Rarely                 | Remove before search                                |
| `〇〇編` (arc name)          | No                      | Often present          | Remove (base title scores higher)                   |
| Roman numerals (`II`, `III`) | Sometimes part of title | Varies                 | Handle cautiously — may be part of the actual title |

## Unicode Normalization

TMDB performs **no Unicode normalization** on search queries (confirmed by Travis Bell).

- `×` (U+00D7 MULTIPLICATION SIGN) and `x` (U+0078 LATIN SMALL LETTER X) are treated as different characters
- `☆` (U+2606) is not normalized or stripped
- Client-side NFKC normalization + custom character mapping is required for reliable matching
