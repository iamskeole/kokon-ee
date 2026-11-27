# 042 — H3 PE refinements: the card's contract, enforced

Date: 2026-11-26. Status: todo.

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

## blockers

- B1 (resolved) — the makers' edit request params are unpublished (the edit
  workflow is "still under development" on the card); the edit schema is ours,
  mirroring their edit response shape.
- B2 (resolved) — per-image vision token cost unknown until a live turn; the
  window measures it (4 and 8 refs) before the ctx is final.

## verification

- (window) the PE boots at the new ctx with the KV pool inside the card; a
  4-ref and an 8-ref edit turn report the per-image token cost; the 16-ref
  budget arithmetic fits.
- (window) t2i and edit replies parse against the strict schema with
  `finish_reason: stop` — the fallback path never triggers in the window.
- (window) the thinking trio is in effect (turn timing/thinking length at
  xhigh vs a `low` A/B row, for the owner).
- (service) an H3 job with a painted mask (sent straight to the API) drops it
  with a note and completes as an edit; the UI's brush is absent for the H3
  entry and present for qwen.
- (planner) K4: H3 on the 24 GB card plans 4096² (16.8 MP) with no reduction
  note; a 16 GB card clamps it; qwen's K4 request clamps to its budget.
- (config) schema 19 round-trips; the H3 entry's flags and list come back
  intact from `/api/v1/config`.
- full suite green: `cargo test -p qls-core`, `npm test`, `cd mcp && npm test`.
