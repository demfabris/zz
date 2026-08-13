---
type: Concept
title: Browser history & omnibox autocomplete
description: Profile-scoped browser history, recent-use ranking, learned selections, and Chrome-like URL-bar autocomplete.
resource: crates/zz/src/browser/recent_pages.rs
tags:
- browser
- history
- omnibox
- autocomplete
- profiles
timestamp: 2026-08-13T01:57:40Z
---

# Overview

zz keeps browser history outside CEF so the native address field can search it
without opening Chromium's internal databases. The history store belongs to the
GUI client, scopes every page and learned selection to a named zz profile, and
feeds both the blank-page recent list and non-empty omnibox queries.

The implementation follows Chromium's stable user-facing rules instead of its
field-trial scoring constants: URL and title fit lead, then explicit selections,
typed use, visit frequency, and recency decide ties. Exact Chromium ordering can
change with field trials and server features, so zz owns a small deterministic
scorer.

# Stored records

`recent-pages` starts with a `v2` marker and contains two tab-separated record
types:

| Record | Fields | Limit |
| --- | --- | --- |
| `p` | profile, last visit, visit count, typed count, last typed use, URL, title | 5,000 across profiles |
| `s` | profile, last selection, selection count, normalized input, destination URL | 5,000 across profiles |

URLs, titles, inputs, profile names, record counts, and the whole file carry
independent bounds. The loader rejects files over 64 MiB. Writes use the app's
atomic replace path and restrict the file to the current user on Unix. A legacy
`unix-seconds<TAB>url<TAB>title` row loads into the `default` profile with one
visit and no typed credit; the next write upgrades the file.

CEF never reads this file. It contains browsing URLs, titles, and learned address
text, so operators should treat it as sensitive user data.

# Recording and profile isolation

Main-frame `AddressChanged` events from active and background tabs update one
exact-URL page record in the pane's logical zz profile. A repeat visit moves the
record to the front, increments its saturating visit count, and keeps its title.
`TitleChanged` fills or replaces the matching title.

An address-bar submission holds its input until Chromium reports that its load
started and then finished. A successful accepted history result increments typed
use and records a shortcut from normalized input to destination; a successful
incrementally edited address increments typed use without a shortcut. A bulk
replacement such as a pasted destination contributes an ordinary visit but no
typed credit. Failed, superseded, or never-started submissions receive no typed
or shortcut credit. Ordinary page navigation still contributes visit recency and
frequency.

The profile key is the descriptor's normalized zz profile, not a temporary
remote-egress composite request context. Switching profiles changes the visible
history and autocomplete corpus. Deleting one suggestion removes that exact URL
and its learned shortcuts from the current profile only.

# Matching and ranking

Queries must contain non-whitespace text. A page qualifies for autocomplete when
it has typed use, at least four visits, or a visit within the last three days.
Every whitespace-delimited query term must occur in the lowercased URL or title.

The deterministic ordering is:

1. exact formatted address;
2. host prefix, with a host-only URL ahead of a deep URL;
3. address or full-URL prefix;
4. URL word boundary, title word boundary, URL substring, then title substring;
5. learned selection strength, typed use, visit frequency, and recency;
6. newest visit and URL as stable tie-breakers.

Repeat counters use logarithmic boosts so a frequently visited old page can beat
a one-off recent page without permanently pinning itself. Recency boosts decay
by age in days. Learned selections record hit count and last selection time, so
choosing the same result for a prefix moves it up on later uses.

# Address-field behavior

The omnibox does not emit ordinary history results for an empty query. A blank
tab keeps its separate eight-row recent-page surface.

For non-empty input:

- up and down wrap through at most eight results and preview the selected URL;
- Enter opens the selected result, or resolves the user's text as an address or
  search when no row is selected;
- Escape restores the original query from a preview, then closes the result
  list, then restores the current page URL and returns focus to the page;
- Shift+Delete removes the selected URL and its learned mappings from the current
  profile;
- pointer selection opens the chosen result.

A one-character append can inline-complete the top URL-prefix result. Root URLs
need one typed use; URLs with a path, query, or fragment need two. The suffix is
selected so the next typed character replaces it. Deletes and multi-character
replacements do not trigger inline completion.

# Chrome import

Explicit **Import Chrome data** reads `url`, `title`, `last_visit_time`,
`visit_count`, and `typed_count` from the selected Chrome snapshot. The current
zz profile remains the destination. Merge keeps the newer visit and usable title,
plus the larger visit and typed counts, so an import enriches current zz history
without weakening locally learned use.

# Key files

| File | Role |
| --- | --- |
| `crates/zz/src/browser/recent_pages.rs` | Bounded storage, migration, matching, scoring, learning, deletion, and tests. |
| `crates/zz/src/browser/view.rs` | Input events, successful-use credit, keyboard selection, Escape stages, and result actions. |
| `crates/zz-ui/src/browser.rs` | Native result panel and title/URL rows under the compact toolbar. |
| `crates/zz-chrome-import/src/history.rs` | Read-only extraction of Chrome timestamps and use counts. |

# Related

- [Input translation](/browser/input-translation.md) owns address-versus-search
  resolution and the surrounding browser key contexts.
- [Browser profiles](/browser/profile.md) define the logical history boundary and
  the `recent-pages` path.
- [Browser lifecycle](/browser/lifecycle.md) supplies the address, title, loading,
  failure, and closed events used to update history.

# Citations

1. [Chromium history significance and URL schema](https://chromium.googlesource.com/chromium/src/+/refs/heads/main/components/history/core/browser/url_database.cc)
2. [Chromium history URL ordering and inline eligibility](https://chromium.googlesource.com/chromium/src/+/refs/heads/main/components/omnibox/browser/history_url_provider.cc)
3. [Chromium indexed URL/title scoring](https://chromium.googlesource.com/chromium/src/+/refs/heads/main/components/omnibox/browser/scored_history_match.cc)
4. [Chromium learned shortcut scoring](https://chromium.googlesource.com/chromium/src/+/refs/heads/main/components/omnibox/browser/shortcuts_provider.cc)
