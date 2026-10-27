# markkit_extract results, 2026-10-27

Run: `just eval` (mira 0.5, `markkit_extract`, both `mode` values) on main after #31 (relative URLs), IR v2, limits (#27) and main-content detection. Raw mira report: [`markkit_extract-2026-10-27.json`](markkit_extract-2026-10-27.json). Design: [knowledge/operations/eval.md](../../../knowledge/operations/eval.md).

## Summary

| mode | pages passing | overall | completeness | accuracy | formatting | noise |
|---|---|---|---|---|---|---|
| `default` | 0/10 | 74% | 94% | 100% | 96% | 34% |
| `main_content` | 3/10 | 87% | 90% | 90% | 88% | 82% |

Overall = mean weighted check pass rate; a page passes only when every check passes.

## Per page (weighted rate)

| page | default | main_content |
|---|---|---|
| news_article | 73% | 100% |
| docs_page | 81% | 90% |
| blog_post | 59% | 100% |
| product_page | 84% | 100% |
| recipe | 75% | 94% |
| wiki_article | 50% | 67% |
| forum_thread | 81% | 62% |
| changelog | 93% | 93% |
| data_table | 75% | 81% |
| legacy_layout | 64% | 86% |

## Failed checks

- **news_article** `default`: not_contains "We use cookies"; not_contains "Refinance today"; not_contains "Advertisement"; not_contains "Related stories"; not_contains "New bike lanes"; not_contains "Get the morning briefing"
- **docs_page** `default`: not_contains "Quickstart"; not_contains "Edit this page"; not_contains "Last updated"; not_contains "Previous:"
- **docs_page** `main_content`: not_contains "Edit this page"; not_contains "Last updated"
- **blog_post** `default`: not_contains "We're hiring"; not_contains "Share on X"; not_contains "Share on LinkedIn"; not_contains "14 Comments"; not_contains "Did you consider Buck2"; not_contains "Log in to comment"; not_contains "Subscribe to our newsletter"
- **product_page** `default`: not_contains "You may also like"; not_contains "Ridgeline 30L"; not_contains "TRAIL10"
- **recipe** `default`: not_contains "Sponsored"; not_contains "MealBox"; not_contains "Chef's knife"; not_contains "Rate this recipe"
- **recipe** `main_content`: not_contains "Rate this recipe"
- **wiki_article** `default`: heading "## Construction"; heading "## Destruction"; not_contains "Log in"; not_contains "Random article"; not_contains "[edit]"; not_contains "From OpenWiki, the free encyclopedia"; not_contains "Contents"; not_contains "Categories:"; not_contains "last edited on"
- **wiki_article** `main_content`: heading "## Construction"; heading "## Destruction"; not_contains "[edit]"; not_contains "From OpenWiki, the free encyclopedia"; not_contains "Contents"; not_contains "Categories:"
- **forum_thread** `default`: not_contains "Viewed 48k times"; not_contains "Hot Network Questions"; not_contains "CSS grid"
- **forum_thread** `main_content`: heading "# How do I reverse a linked list in place?"; contains "reverse it without allocating a new list"; contains "recursive version also works"; order "without allocating a new list" before "You overwrite"; order "O(1) extra space" before "recursive version"
- **changelog** `default`: not_contains "Start your free trial"
- **changelog** `main_content`: not_contains "Start your free trial"
- **data_table** `default`: contains "Total"; contains "72.4"; contains "Quarterly generation"; not_contains "Share:"
- **data_table** `main_content`: contains "Total"; contains "72.4"; contains "Quarterly generation"
- **legacy_layout** `default`: not_contains "MENU"; not_contains "Guestbook"; not_contains "visitor number"; not_contains "Netscape Navigator"
- **legacy_layout** `main_content`: not_contains "visitor number"; not_contains "Netscape Navigator"
