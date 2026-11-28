# task-049: one job, two progress clocks — the card and the strip frame disagree (upstream scope)

**Parked (owner trigger; his side-quest from 2026-11-28, ~03:35).** Screenshot evidence at
03:16: the main stage card shows **20 % "Generating"** while the timeline strip frame for the
*same job* shows **90 %**. The owner: "these should reference the same algorithm / math /
estimate; if we figure out this is some funky business in code and we find a fix for it, I
think we should scope out just this bit into a [PR] for upstream so we help them too."

## the mechanism (verified by code reading at setup time)

Both numbers are the same function — `jobPercent(job, clock)` in `src/lib/logic.ts`,
`elapsed / (elapsed + jobSecondsLeft)`, capped at 99 — but every UI element builds **its own
private `JobClock`** inside `src/components/jobMotion.ts::useJobPercent`, and the clocks
start at different moments:

- **The main card** (`Stage.tsx :: Pending`, `useJobPercent(job, request, learn = true)`)
  uses the persistent `cardMemory(job.id)` — but the card **only mounts while that version is
  on the stage**: the code comment says it, "Looking at another version unmounts the card;
  coming back continues from where it is." Its clock's `startedAt` is set by the first
  observation of that memory — if the card first mounted mid-job (the viewer was looking at
  another version), the elapsed excludes everything before (queue wait, backend start, model
  load, prompt enhance).
- **The strip frame** (`Stage.tsx :: JobPercent`, line 961, `learn = false`) gets a
  **fresh `CardMemory` per mount**, and it only renders while `job.status !== "queued"` —
  so its clock starts when the job leaves the queue and counts the whole front through.
- `StickerRing` and `ShadowPhase` in `LayerDoc.tsx` also use `learn = false` — same
  exposure.

So at 03:16: the strip had been counting since the job left the queue (load + enhance +
generating so far) against a small remaining estimate → 90 %; the card had only been
mounted since generating began (or since the viewer returned to it) → small elapsed, large
remaining → 20 %. Same job, same math, **two private clocks that never agreed on when the
clock started** — which is exactly why the owner's hunch ("probably only or mostly related
to queued jobs") is right: the bigger the queue/load front, the bigger the split.
Compounding, by design: `c.target = Math.max(c.target, …)` (never goes down) and the
0.18/tick easing (a returning card chases its target for a second or two).

## the fix (direction; decide in the session)

- **One clock per job, shared by every consumer:** `useJobPercent` always uses
  `cardMemory(job.id)` (drop the fresh-memory branch), so card, strip frame, and sticker
  ring all read the same `startedAt`/`runningAt`/`phase`/`step` timestamps — first
  observer sets them, everyone after reads them. The numbers then agree by construction
  (the easing lag remains, briefly, by design).
- **Anchor to the server's wall clock, not the viewer's first glance** (the stronger
  fix): the service knows when the job actually left the queue and when each phase began
  (the engine already tracks the step view in `engine.rs`). If the job view carries an
  absolute start (and cheaply, phase timestamps), the client can compute true elapsed even
  when no UI element has looked at the job since submission. Check what `JobView`
  (`src/lib/types.ts`) and the engine's job view already carry before adding fields.
- Keep the monotone target and the easing — "only ever going up, counts smoothly" is a
  deliberate UI choice, not part of the bug.
- **Regression test (vitest, no GPU):** drive a synthetic timeline (queued →
  starting_backend → enhancing → generating with steps) through both call paths — a card
  that mounts late and a strip that mounts at queue-departure; assert the divergence
  reproduces before the fix (the 20/90 class) and that the two match within ±1 after.

## upstream scope (the owner's ask)

The fix lives in `src/components/jobMotion.ts` (+ the call sites in `Stage.tsx` /
`LayerDoc.tsx`) — stock upstream Kokon UI code, no service or engine entanglement. If the
stronger fix needs a job-view field from the service, decide in the session whether that
small addition rides in the upstream PR or stays fork-side. House rules apply: plain
English in comments, BFL prompt strings never in a diff, one small commit per unit.

## acceptance criteria

- [ ] Card, strip frame, and sticker ring show the same percentage (±1, easing allowed)
  for one job across mount/unmount, regardless of when the viewer first looked at it.
- [ ] The vitest regression reproduces the divergence before and the agreement after.
- [ ] The change is scoped as an upstream PR (the owner decides when it goes out); the
  fork's diff stays minimal.

## blockers

None. Separate session on the owner's trigger.
