# Self-Healing Adversarial Test Matrix

Scoped to the fuzzy-match alias-discovery mechanism in `orchestrator/src/rdf.rs`
(`find_alias_candidate` / `normalize`) and its interaction with the persistent
alias registry in `orchestrator/src/shim.rs`. The question these 16 scenarios
were trying to answer: **when does "this looks like a renamed field" stop
being a safe inference?**

**Status: all 16 scenarios now have a test or an implemented mitigation.**
14 run as automated tests (16 unit tests in `rdf.rs`/`shim.rs` + 2 `#[ignore]`
integration tests in `shacl.rs` requiring live Fuseki, run via
`cargo test -- --ignored`). E1 (temporal collision) doesn't have — and
structurally can't have — a pass/fail test, since "has this alias gone
stale" isn't mechanically decidable; it got an audit-trail mitigation
instead. F2 was a real latent bug, now fixed and regression-tested.

Two real findings came out of writing the tests, not just reading the code:
- **B2** (conflicting-value collisions) was unhandled — the matcher would
  have silently picked whichever candidate sorted first. Now detected and
  refused rather than guessed.
- **F2** (stale `used` set mid-loop) was a genuine bug, just not one the
  current 5-field schema could expose. Fixed regardless.

## Background: how a collision could happen

`normalize()` lowercases and strips everything but alphanumerics before
comparing a missing canonical field's name against the event's actual keys.
That's deliberately loose — `user_id`, `USER-ID`, and `User.Id` all normalize
to `userid` and match `userId`. The looseness is the point (it's what makes
the matcher useful) but it's also the entire attack surface: anything that
normalizes to the same string as a canonical field is treated as that field,
regardless of whether it actually is.

Two structural facts bound the risk today:
- The 5 canonical fields (`eventId`, `userId`, `trackId`, `timestamp`,
  `msPlayed`) all normalize to *distinct* strings, so no two canonical
  fields can currently be confused for each other.
- Matching is name-only — the candidate's *value* is never inspected during
  discovery. This is safer against value-based coincidence but means a
  structurally-valid-looking key with garbage content still gets healed
  (confirmed by D1).

## Summary

| # | Category | Scenario | Primary risk | Status |
|---|---|---|---|---|
| A1 | Naming variant | snake_case rename | baseline (control) | ✅ `rdf::tests::a1_snake_case_rename_discovered` |
| A2 | Naming variant | UPPER/mixed-case rename | false negative if case handling regresses | ✅ `rdf::tests::a2_case_variants_all_match` |
| A3 | Naming variant | separator variants (`-`, `.`) | false negative | ✅ `rdf::tests::a3_separator_variants_all_match` |
| A4 | Naming variant | abbreviation that should *not* match (`uid`) | false positive (over-matching) | ✅ `rdf::tests::a4_abbreviation_does_not_match` |
| B1 | Same-field ambiguity | two candidate keys, same value | nondeterminism (benign) | ✅ `rdf::tests::b1_multiple_candidates_same_value_resolves_unique` |
| B2 | Same-field ambiguity | two candidate keys, conflicting values | wrong data silently chosen | ✅ **fixed + tested** — `rdf::tests::b2_multiple_candidates_conflicting_values_ambiguous` |
| B3 | Same-field ambiguity | canonical key + stale decoy alias | precedence correctness | ✅ `rdf::tests::b3_canonical_key_takes_precedence_over_decoy_alias` |
| C1 | Multi-field drift | two fields renamed at once | compounding repair correctness | ✅ `rdf::tests::c1_two_fields_renamed_simultaneously` |
| C2 | Multi-field drift | all four required fields renamed | worst-case single-pass load | ✅ `rdf::tests::c2_total_drift_all_required_fields_renamed` |
| C3 | Multi-field drift | one repairable + one truly-missing field | partial-heal correctness | ✅ `rdf::tests::c3_mixed_repairable_and_unrepairable_in_one_event` |
| D1 | Value independence | name matches, value is garbage | false confidence — heals structurally, data is junk | ✅ **confirmed real** — `shacl::tests::d1_garbage_value_with_matching_name_still_conforms` (live Fuseki) |
| D2 | Value independence | value shape coincidentally fits wrong field | type-coercion edge case | ✅ `shacl::tests::d2_non_numeric_ms_played_is_lexically_invalid` (live Fuseki) |
| E1 | Temporal collision | learned alias key later reused for a different meaning | silent permanent misinterpretation | ⚠️ **mitigated, not testable** — audit trail (`learned_at`/`last_used_at`) added; see below |
| E2 | Temporal collision | same drifted key across many events post-learning | idempotency / log-noise | ✅ `shim::tests::e2_learning_same_alias_twice_is_idempotent` |
| F1 | Structural edge case | duplicate JSON keys in one object | undefined-behavior surface | ✅ `rdf::tests::f1_duplicate_json_keys_last_value_wins` |
| F2 | Structural edge case | stale `used_raw_keys` across multi-field repair in one pass | cross-field mismatch bug | ✅ **fixed + tested** — `rdf::tests::f2_used_keys_threaded_across_fields_in_one_pass` |

Restart durability (alias survives a process restart) also has a dedicated
test now — `shim::tests::e1_alias_survives_reopen` — separate from the E1
temporal-collision scenario itself, which is about a key being *repurposed*,
not about restart persistence.

Run: `cargo test --manifest-path orchestrator/Cargo.toml` for the 16 unit
tests, then `cargo test --manifest-path orchestrator/Cargo.toml -- --ignored`
for the 2 live-Fuseki integration tests (needs `docker compose up -d jena`
with the ontology/shapes graphs loaded — see STATUS.md for the curl
commands).

---

## Category A — Single-field naming variants

Baseline for `normalize()` itself. All four confirmed exactly as expected —
case and separator variants heal, the abbreviation negative control doesn't
match. No surprises, no changes needed.

## Category B — Multiple candidates for one missing field

**B1 — two candidates, same value.** Confirmed `find_alias_candidate`
resolves to `Unique` and picks deterministically (alphabetically-first, since
`serde_json`'s default non-`preserve_order` `Map` iterates in sorted key
order — this was an assumption in the original write-up, now pinned down by
the test rather than just observed).

**B2 — two candidates, conflicting values. Real gap, now fixed.** Before
this pass, `find_alias_candidate` returned a single key with no way to
signal "there were multiple, and they disagreed." Fixed by introducing
`AliasMatch::Ambiguous(Vec<String>)`: when more than one raw key
normalize-matches a missing field *and* their values differ, the matcher now
refuses to pick one, and `validate.rs` logs it as
`"multiple candidate keys with conflicting values — refusing to guess, not
healed"` instead of silently writing one of them. The field stays missing
(same outcome as an unrepairable breach) rather than risking wrong data with
zero error signal.

**B3 — canonical key plus a stale decoy alias.** Confirmed: `extract()`'s
candidate list always tries the canonical name first (index 0 in the alias
vec seeded by `default_aliases()`), so canonical wins over any learned alias
regardless of alias-list order.

## Category C — Multiple fields drifting in the same event

All three confirmed. C3 in particular confirms healing is per-field, not
all-or-nothing at the row level: one field can resolve via fuzzy-match while
a genuinely-absent field in the same event stays missing.

## Category D — Value content is never checked during matching

**D1 — confirmed as a real, currently-accepted weakness**, via a live Fuseki
run rather than just code inspection: an empty `userId` correctly fails
`sh:minLength 1`, but a non-empty garbage string (`"!!!not-an-id###"`)
conforms cleanly. Name-matching only proves a key exists and looks shaped
right — it says nothing about whether the content is meaningful. Not fixed;
this is a scope boundary worth knowing about rather than a bug (fixing it
would mean adding value-plausibility heuristics, a different and much
riskier kind of "guessing" than name-matching).

**D2 — confirmed the two layers agree.** A non-numeric `msPlayed` value gets
flagged by SHACL's `xsd:nonNegativeInteger` lexical-validity check. (The
DuckDB-side `TRY_CAST` behavior for the same value — silently returning
`NULL` rather than erroring — was documented in the original write-up but
isn't separately asserted by a test; SHACL rejecting the event before it's
considered "healed" makes the DuckDB-side behavior moot in practice.)

## Category E — Collisions across time, not just within one event

**E1 — mitigated, not tested.** This is the one scenario in the matrix that
doesn't reduce to a pass/fail assertion: nothing in the data itself
distinguishes "this alias is still correct" from "this key got silently
repurposed for something else." Automatically expiring or revoking an alias
risked being worse than the disease (a heuristic guessing *when* a mapping
went stale is just a second unreliable guess layered on the first). Instead,
`shim_aliases` now has `learned_at` and `last_used_at` columns
(`ShimEngine::touch_alias`, called from `validate.rs` every time a field
resolves via a non-canonical key), and `ShimEngine::alias_audit()` exposes
them. This turns an invisible risk into an operationally-checkable one — an
operator (or a future scheduled check) can look for aliases whose
`last_used_at` stopped advancing, or that get touched with suspiciously
different-looking values, without the system trying to auto-decide staleness
on its own.

**E2 — confirmed idempotent.** Re-learning the same `(canonical, raw_key)`
pair returns `false` on the second call (vs. `true` on the first) and
produces no duplicate `shim_aliases` row, backed by the table's
`PRIMARY KEY (canonical, raw_key)` + `INSERT OR IGNORE`.

## Category F — Structural / implementation edge cases

**F1 — confirmed: last-value-wins.** `serde_json` resolves duplicate JSON
keys by keeping the last occurrence. Documented as a fact now, not an
assumption.

**F2 — real bug, now fixed.** `validate.rs`'s repair loop was computing
`used_raw_keys` once before the loop and never updating it as earlier
iterations of the *same* pass learned new aliases. This couldn't produce a
wrong result with today's 5 canonical fields (none of them normalize to the
same string, so nothing could actually double-claim a key) — but that was a
property of the current schema, not something the code enforced. Fixed by
cloning `used` into a local mutable set and inserting into it after every
successful match within the loop, so a 6th field added later can't silently
reintroduce this. Regression-tested with two contrived colliding field names
(`fooId`/`foo_Id`) since the real ontology can't exercise the bug directly.

---

## What's left

Nothing pending from this pass. If the ontology grows beyond the current 5
fields, re-run `f2_used_keys_threaded_across_fields_in_one_pass`-style
thinking against the new field set — that's the one guarantee that was
"correct by coincidence" before this pass and is now enforced by code, but
it's still worth a fresh look whenever the schema changes shape.
