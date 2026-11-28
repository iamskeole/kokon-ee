# task-045: measured models are trusted for edits too — the 16 GB calibration loses its say

Opened 2026-11-27, late, from the owner's review of the H3 edit numbers (his 4K edits
rendered at 2304² then enlarged — the "upscaled/original" pills).

## the problem

The planner's `budget_area` has two brains. For a model row carrying
`measured_peak_mib` (H3: 23 552 MiB measured on this 24 GB card), a card that holds the
peak trusts the measured tiers — **but only for reference-free jobs**
(`inputs.image_sizes.is_empty()`). The instant a reference is attached (every edit), the
planner dropped the measurement and fell back to the Qwen-calibrated token budget
(16384 tokens anchored on "a 16 GB card runs 2048²" → 24 832 tokens ≈ 6.1 MP on this card,
references counted at ≤1 MP). Result: H3 balanced/quality edits clamped to 2304² (5.3 MP)
while the same card ran 16 MP generates at 22.3 GB — the clamp was the formula, not the
card.

Measured on this card: 16 MP generate fits (22.3 GB); 2304² render + 1 MP reference canvas
ran clean twice (23:00/23:02); the only true OOM ever measured is 16 MP render + 1 MP
reference canvas + 4K decode (B4, the door died). The region between 5.3 MP and 16 MP for
edits had never been tested.

## decisions

- **D-045-1 (owner):** "I'd like the service to stop interfering with the backend
  altogether, e.g. trust it; do the same as generate, no funny business. … I don't want
  the original calibrations that don't really have anything to do with a box owner that
  manages their own backends to have a say in how they operate the backends." → the
  measured-peak bypass no longer requires reference-free jobs: for a row with a measured
  peak, a card that holds the peak budgets **every job shape** (generate, edit, outpaint,
  combine) at `area_quality`. The reference canvases still count against the tier — they
  live in the DiT's context, and the owner sizes them with the reference detail
  (Standard 1 MP / Full). Cards that cannot hold the measured peak keep the generic token
  budget (the safety valve is unchanged). Only models with measured rows are affected
  (H3 today; Qwen/Klein rows carry none and keep the 16 GB calibration). No per-model code
  — the row is config data (D-044-12 discipline).
- **D-045-2 (owner's test plan):** if the new-quality edit (≈15 MP render + 1 MP reference
  canvas + 4K decode — just under the one known OOM point) OOMs, the door dies and the
  daemon revives it, and the service retreats to a conservative per-model edit area
  (proposed: an `editMaxArea` row field at 8.5 MP: balanced edits native, quality edits
  render 8.5 MP and enlarge 1.41×). If it survives, H3 edits are native at every preset.

## acceptance criteria

- [x] `planner.rs`: the measured-peak bypass applies to every job shape; the comment and
  the `h3_tiers_stand_on_their_measured_peak` test re-state the behavior (a quality edit
  with a 4K reference on the 24 GB card renders at (or within the 8 % grace of) the 16 MP
  tier — not the old ~5.3 MP clamp).
- [x] The "Resolution reduced" note only stays when the canvas actually shrank below the
  target (the 8 % grace had made a true 4096² render announce a reduction that did not
  happen); `quality_edit_shares_the_2k_budget_with_its_reference` still holds.
- [x] `cargo test -p qls-core` green (403).
- [x] Service image `kokon-ee:dev-h3` rebuilt and the `kokon-ee` container re-created.
- [x] Live `/api/v1/plan` check: balanced edit (4K reference) = native 2896² tier, no
  clamp note; quality edit = native 4096², zero notes.
- [x] Owner's test: H3 balanced/quality edits through the live UI. Verdict decides
  D-045-2 (stand, or retreat to the conservative edit area).

## outcome (2026-11-28, ~01:30 UTC)

**D-045-2 resolved: it stands.** The owner's 4K quality edit (4096² render + 1 MP
reference canvas + full 4K reference decode) ran to completion; nvidia-smi at the
peak showed the sd-server at **22 572 MiB of 24 576** (~2 GB of headroom, no OOM,
no door death). The job's wall time was 280.8 s (gallery `duration_ms`, door boot
included). So H3 edits are **native at every preset** on this card — no
conservative `editMaxArea` was needed; the only measured OOM remains the 16 MP
render + *full* 4K reference (B4), which the reference detail control already
keeps out of the way (Standard = 1 MP canvas).

## blockers

None.
