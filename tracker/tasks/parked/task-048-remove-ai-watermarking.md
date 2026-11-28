# task-048: remove AI watermarking (and all AI marking) from the code

**Parked (owner trigger, separate session).** Set up 2026-11-28, ~01:15, while the owner was
about to run the turbo/H3 tests — captured so it can't slip.

## decisions

- **D-048-1 (owner, this session):** "Remove any AI watermarking from the code: on one hand,
  we don't want to distort outputs of the models themselves, and on the other hand we go by the
  principle of trusting the user to know what they're doing — we don't babysit them." Scope,
  the owner's words: **all paths that (a) apply anything on top of model outputs** (the
  invisible pixel watermark), **(b) metadata** (the IPTC Digital Source Type / provenance
  written into files), **(c) UI references to watermarking that will be irrelevant once the
  functionality is removed.**

## background / inventory (verified against the tree at setup time)

The feature is the EU AI Act Art. 50(2) marking described in `docs/ai-marking.md`: every image
the service makes or changes is marked **twice** — an **invisible watermark in the pixels**
(embedded by the TrustMark-style model, BCH-coded payload) and **IPTC "Digital Source Type"
metadata** in the file. ~1,850 lines of core code plus the wiring around it:

- **Core modules:** `crates/qls-core/src/watermark/` (`mod.rs` 438 ln — `embed()`/`detect()`;
  `bch.rs` 1,085 ln — the error-correcting code) and `crates/qls-core/src/provenance.rs`
  (329 ln — the IPTC source-type words and read/write; no C2PA crate dependency — hand-rolled).
- **Pipeline wiring:** `engine.rs` — `watermark::embed` into the pixels of every copy an
  output is made from (the "every copy" site, ~line 2779) and the failure note ("The invisible
  AI watermark could not be added (its model is missing or failed); the file's metadata still
  marks it as made with AI."); the `detect`-based check; the recurring
  "no AI watermark …: The AI watermark model is not installed" warning (the box has never
  had the model installed — every job record carries that note).
- **Storage:** `store.rs` — the gallery's display copy PNG carries the provenance/marking.
- **API:** `/provenance/check` route (`api.rs`) + the report shape in `src/lib/api.ts`
  (`watermark: { checked, found, kokon, text, schema }` + the `ai` flag).
- **UI:** `src/components/AiCheck.tsx` (~144 ln — the "Check an image" panel: what an image's
  markings say) and its card in `Settings.tsx` ("AI marking"); `App.tsx`/`api.ts` references.
- **Model + build:** the watermark model pin in `catalog.rs` (the missing download behind the
  recurring note); `build.rs` references; `examples/watermark_check.rs`.
- **Docs:** `docs/ai-marking.md` (the whole doc), mentions in `README.md`,
  `docs/engineering-notes.md`, `docs/fresh-box.md`, `docs/http-api.md`,
  `docs/service-on-prod-box.md`, `CONTRIBUTING.md`, the AGENTS.md doc table.
- **Tests:** the engine tests around the marking (the `KOKON_TEST_MODELS` watermark-folder
  assertions, "a LUT of a camera photo is not AI-made" etc.) and whatever the UI logic tests
  cover of the check panel.
- **Not in scope by inspection:** the MCP server has no watermark references of its own
  (job notes flow through opaquely); the 125 total matches include tracker history (done
  tasks) which is never rewritten.

## acceptance criteria

- [ ] No path in the pipeline applies anything to pixels after the model's decode
  (no `watermark::embed`; the output file is the model's raw decode, exactly as the doors
  return it), and no metadata marking is written into outputs (no IPTC source-type writes).
- [ ] The `watermark/` module, `provenance.rs` (or the parts that survive the open questions
  below), the check route, the UI card/panel, the config/catalog pins, and the note strings
  are gone; no dead references left in `crates/`, `src/`, or `mcp/`.
- [ ] Docs: `docs/ai-marking.md` deleted and the doc table/links updated; the remaining docs
  state plainly that outputs are the models' unmodified results and nothing is marked.
- [ ] Tests: the marking tests removed/replaced; suites green (qls-core / UI / mcp); the
  recurring "watermark model not installed" note no longer appears in job records.
- [ ] House rules: one small commit per unit; no behavior change beyond the removal; the
  gallery's **existing** marked images are left untouched on disk (prospective removal only —
  scrubbing user files would be its own babysitting).

## open questions (owner decides in the session)

- **Q1 — RESOLVED (owner, 2026-11-28, same session):** remove **all** of the check side
  too, including the generic third-party reader — "fine to remove it all, that's not our
  business." The `AiCheck` panel, the card in Settings, `/provenance/check`, and the report
  shape in `api.ts` all go.
- **Q2 — the watermark model files on the box** (`models/…` in the ml repo): leave on disk
  (harmless) or delete in the ml repo. Default: leave; they are the ml repo's business.
- **Q3 — `docs/ai-marking.md` →** delete outright vs. a one-paragraph "we mark nothing"
  note. Default: delete; the README gets one plain sentence instead.

## blockers

None. Separate session on the owner's trigger.
