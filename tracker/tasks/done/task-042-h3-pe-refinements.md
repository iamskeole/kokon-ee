# 042 — H3 PE refinements: the card's contract, enforced

Date: 2026-11-26. Status: done (2026-11-27, window models-042-h3).

The 041 close-out review (the owner) turned up seven gaps between what we built
and what the makers' PE card and the pinned toolchain actually offer:

1. **The H3 contract is keyed on the model-id substring `h3`** (`enhancer::h3_pup`,
   five call sites) instead of on the loaded bundle's family (`minimax-h3`). A
   renamed door silently loses the contract. Key the whole contract (rec row,
   PE-requirement check, request assembly, decode) on the bundle family, the way
   the loader already routes it.
2. **The brush shows for H3.** `modelSupportsInpaint` is a negation
   (`!modelSupportsBoxes`), so every non-klein model gets the mask tool — but the
   H3 card has no mask/inpaint path (text-to-image + reference editing only; the
   maker's API takes 1–16 reference images, no mask). The picker entry needs a
   capability flag (`supportsInpaint`) and the UI hides the brush when it is
   false; the engine drops a mask defensively with a note.
3. **The resolution list is global** (auto/source/1k/1.5k/2k/custom) — qwen's
   native 2K ceiling. On H3 an explicit choice caps below the measured 16 MP
   native tier, and there is no "native" option. Per-entry `resolutions`
   (deployment data, default = the global list): H3 gets `1k/2k/4k` (new
   `ResolutionMode::K4` = 4096², the measured native tier); qwen stays 1k/1.5k/2k
   (their native 2K buckets; their doors report 4096 max but the card's native is
   2K), klein as today.
4. **The PE reply is prompt-enforced, not grammar-enforced.** The makers'
   SGLang deployment constrains decoding with a strict JSON schema (xgrammar);
   the pinned llama.cpp (52975342f) takes a per-request `json_schema`
   (converted to GBNF, output-format scoped — thinking stays free). Send the
   makers' T2I schema (verbatim, from their `request_params.json`) and the
   matching edit schema per request. The bundle gains both files (hash-verified);
   the out-of-contract fallback path stays as the last resort.
5. **The card's thinking parameters never reach the template.** The card sets
   `reasoning_effort: xhigh` + `enable_thinking`/`preserve_thinking`; llama.cpp
   takes per-request `chat_template_kwargs` (the qwen3.8 template accepts
   `reasoning_effort` ∈ xhigh/medium/low — the same mechanism this host's own pi
   config uses, `PI_REASONING_LEVEL`). Send the card's trio per request from the
   bundle's params files.
6. **The PE context (32768) does not hold 16 references + the thinking budget.**
   The maker's API takes up to 16 reference images; the PE turn must fit
   16 × (per-image vision tokens) + system + task + instruction + 16k thinking.
   Measure the per-image cost live and size `-c` (candidate 65536) so the KV
   pool still fits the 24 GB card with the 17 GB weights.
7. **MTP / speculative decoding on the PE.** Checked and confirmed absent —
   not a quant gap. The makers' own PE weight map (HF, bf16) lists 2387 tensors
   and **zero** mtp/nextn/eagle ones: the fine-tune was released without the
   MTP head (mradermacher's Q4_K_M likewise has none; so do the 9B qwen PE
   doors and the Qwen3.5-9B base). The pinned llama.cpp does implement
   qwen35 MTP (the nextn tensors + `draft-mtp`), but `qwen35.cpp:512`
   hard-asserts `n_layer_nextn > 0` — `--spec-type draft-mtp` on a headless
   file **aborts the door at model init** rather than degrading, so the flag
   must not be added until a head exists. The only live MTP on the box is
   vLLM 033 (the makers' *base* Qwen3.8-27B, which does ship the head, with
   vLLM's own spec implementation). The PE stays at the measured ~26 tok/s
   for now (the owner: "c'est la vie"); the speed cost is bounded — a PE turn
   is 33–38 s of a 100–140 s job. **Parked trigger:** an MTP-carrying PE (the
   makers publishing the head, or a quant of it) re-opens this — boot flag
   `--spec-type draft-mtp --spec-draft-n-max 2`, then measure.

## decisions

- D-042-6 — MTP confirmed absent from the model itself (0/2387 tensors in the
  makers' PE weight map; 0 in every quant on disk; the pinned build's qwen35
  MTP path hard-asserts the head, so the flag is a door crash, not a no-op).
  Not enabled; parked on the makers shipping a head.

- D-042-1 — The contract is the bundle's family. `h3_pup(model)` goes away;
  the engine resolves the family from the entry's bundle manifest (cheap
  manifest-only peek where no load is happening yet) and dispatches
  bfl / minimax-h3 / qwen-native on it. The model id never decides.
- D-042-2 — Capability flags live on the picker entry (deployment data):
  `supportsInpaint` (default: the existing box rule — true except klein) and
  `resolutions` (default: the global list). H3 entry: `supportsInpaint: false`,
  `resolutions: ["auto","source","1k","2k","4k"]`. Config schema 18 → 19.
- D-042-3 — The PE reply is grammar-constrained per request: the bundle's
  `t2i/request_params.json` (the makers' file, verbatim: sampling + the strict
  `expanded_caption` + `aspect_ratio` 10-value schema) and
  `edit/request_params.json` (ours, same shape with `target_aspect_ratio`).
  Both gain sha256 in the manifest; the service sends the schema as
  `json_schema` and the card's thinking trio as `chat_template_kwargs`.
- D-042-4 — `-c 65536` on the PE door (measured in the window; 49152 if the KV
  pool does not fit), `--image-min-tokens 1024` per the server's own warning.
- D-042-5 — No MTP on the PE this round (no fitting quant); parked trigger,
  recorded, not litigated.

- D-042-7 — The thinking trio and the strict schema travel per request as the
  pinned llama.cpp wants them: `chat_template_kwargs` with the card's
  `enable_thinking` / `preserve_thinking` / `reasoning_effort`, and `json_schema`
  (the makers' T2I schema verbatim, our edit schema mirroring it). The bundle's
  `t2i/request_params.json` and `edit/request_params.json` both gain sha256 in
  the manifest. This is a request-level constraint (GBNF, output-format scoped),
  so the thinking stream stays free and only the reply is grammar-bound.
- D-042-8 — `chat_template_kwargs` values must keep their native JSON types.
  The first field run 400'd in 1.3 s: the pinned build rejects
  `enable_thinking` as the *string* `"true"` (it wants the boolean). `H3RequestParams`
  and `VlmTurn` therefore carry `BTreeMap<String, serde_json::Value>` (not
  `String`), and `request_params_of` copies the values verbatim. Regression test
  `h3_request_params_keep_the_native_json_types_of_the_template_kwargs` pins it.
- D-042-9 — The capability flags are schema 19 on `ExternalModel` (the picker
  entry), `resolutions: Option<Vec<ResolutionMode>>` and
  `supports_inpaint: Option<bool>`, both `skip_serializing_if = None` so the
  deployed JSON stays minimal. The H3 entry carries `resolutions:
  ["auto","1k","2k","4k"]` and `supportsInpaint: false`. `ExternalModel` gains a
  `Default` derive (all-None/empty) so test literals and the UI can omit them.
- D-042-10 — `ResolutionMode::K4` (4096², H3's measured 16 MP native tier) joins
  the enum, `resolution_area`/the OOM ladder learn it, and the UI's global list
  plus the per-entry lists are the only thing that changes per model: qwen keeps
  auto/1k/1.5k/2k/source, H3 gets auto/1k/2k/4k, klein as today. The biggest tier
  is labelled "native size" for H3.
- D-042-11 — A no-inpaint entry receiving a `mask`/`marks` through the raw API
  drops both at job start (engine-side, before planning) and appends the note
  "This model has no inpaint pass (its editing is text and reference images):
  the marked area was not applied and the image was edited as a whole." so the
  job completes as a whole-image edit rather than erroring. The UI hides the
  brush for such entries (`entrySupportsInpaint`), but the API path is defended
  independently.
- D-042-12 — The PE door runs at `-c 65536` (the card's 16-reference maximum +
  the thinking budget) with `--image-min-tokens 1024`. Measured in the window:
  the KV pool fits the 24 GB card at that context (PE resident at 21456 MiB),
  and a 16-reference edit turn consumed 41311 prompt tokens + 1425 generated
  and finished `stop` in 117 s — the 65536 context, not the 49152 fallback, is
  what was exercised and it held.
- D-042-13 — The per-image vision cost is ~2.0–2.4k tokens at 1024 min-tokens
  (edit1 10503 → edit4 17637 → edit16 41311 prompt tokens), so 16 refs ≈ 41k
  prompt tokens, comfortably inside 65536 even with the thinking budget. The
  `--image-min-tokens 1024` floor (not the lower default) is what keeps a
  small reference from collapsing below the cost the budget assumed.
- D-042-14 — The `/models/switch` endpoint now takes the full `ExternalEngine`
  body, not the 041-era bare `{"model": label}`. The 041 window's switches were
  actually failing silently (400, bare label) — the arm-B "switch" never changed
  the active engine, which is the true mechanism behind D-041-21 (arm-B
  unreachable). The window's `svc_switch` now builds the full body (gateway +
  key from the active engine, model/modelName/sample/vlm/vlmVision/promptBundle
  from the picker entry) and asserts HTTP 200, so a failed switch fails the
  window instead of passing over it.

## blockers

- B1 (resolved) — the makers' edit request params are unpublished (the edit
  workflow is "still under development" on the card); the edit schema is ours,
  mirroring their edit response shape.
- B2 (resolved) — per-image vision token cost unknown until a live turn; the
  window measures it (4 and 8 refs) before the ctx is final.

## verification (all met, window models-042-h3, 2026-11-27 02:13–02:31 UTC)

- (window) the PE boots at `-c 65536` with the KV pool inside the card (resident
  21456 MiB); the edit ladder measured the per-image cost: edit1 10503, edit4
  17637, edit16 41311 prompt tokens (D-042-13); the 16-ref turn finished
  `stop` in 117 s — the 65536 context held, the 49152 fallback was never needed.
- (window) every PE reply (2 t2i + 3 edit) parsed against the strict schema
  with `finish_reason: stop` and `in_contract: true` — the out-of-contract
  fallback never triggered once.
- (window) the thinking knob is live: the card's `xhigh` turn thought 3480 chars
  / 1070 gen tokens in 44 s; the same prompt at `reasoning_effort: low` thought
  1863 / 701 in 30 s — the per-request `chat_template_kwargs` reaches the
  template. (First attempt 400'd on a stringified boolean — D-042-8.)
- (window, service) an H3 edit job sent straight to `/api/v1/jobs` with a
  painted mask (no mask field in the UI path) dropped the mask with the D-042-11
  note and completed as a whole-image edit (done, 212 s, enhanced 1640 chars);
  the H3 t2i job at the new `4k` tier came back `done` at 4096×2720 (3:2 from
  the PE), enhanced 1451 chars. The picker entry's `supportsInpaint: false`
  hides the brush in the UI (`entrySupportsInpaint`), qwen's stays on.
- (planner) K4: H3 on the 24 GB card plans the 4096² native tier with no
  reduction note; the `measured_peak_mib` waiver (041) still stands; a 16 GB
  card clamps; qwen's K4 request clamps to its shared budget. Tests:
  `h3_tiers_stand_on_their_measured_peak`, `k4_is_the_h3_native_tier` (planner),
  the mask-drop test (engine), `schema_19_picker_entry_capabilities_roundtrip`
  (config).
- (config) schema 19 round-trips; the H3 entry's `supportsInpaint: false` and
  `resolutions` come back intact from the live `/api/v1/config` (camelCase);
  the picker sub-line is now "slowest, best for high res images".
- (ui) `resolutionOptions` returns the entry's list for H3 (auto/1k/2k/4k) and
  the standard list otherwise; `entrySupportsInpaint` honours the override
  (H3 off, klein off, qwen on, explicit-true stands). `Composer`/`SizeMenus` use
  both; `App.tsx` passes the full `models` array through (App.tsx:169).
- full suite green: `cargo test -p qls-core` 398 passed; `npm test` 247 passed;
  `cd mcp && npm test` 49 passed (3 skipped); `tsc --noEmit` clean.
