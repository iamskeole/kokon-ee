# task-032 — projects to the database (the recorded D-MU13 trigger)

The owner's recorded trigger (2026-11-13, in the task-026 file): the
JSON-vs-DB question for the projects store is revisited "when the fork
drops its shadow of upstream (D-RB) — when there is no stock format left
to stay byte-identical to, the file constraint disappears and the DB is
the straight answer" — or when multi-user projects outgrow read-the-
whole-file, whichever comes first. **Both halves of the trigger have now
fired** (independence = D-RB executed, task-029; the byte-identity
constraint retires with the stock path, task-031). This task is the
follow-through.

## background (as recorded, 2026-11-13)

- The projects list lives in `<data>/projects.json` — the stock
  0.11+ mechanism (each device sends its copy once, the service merges:
  the `known`-set three-way merge, tombstones for deletions, the rev
  counter). It was kept as a file (not the database) for one reason:
  it is the product's user-visible format and the stock path had to
  stay byte-identical to upstream's, so D-MU13 added a **parallel
  `owner` map** in the same file (skipped while empty, so a
  single-user file never changes) instead of moving the data.
- The rejected alternatives, as recorded: (a) the data in the DB breaks
  the format; (b) a DB table beside the file = two sources of truth.
  Both rejections depended on the stock-format constraint — which
  tasks 029/031 remove.

## acceptance criteria

1. **The design is recorded before the code**: the schema-versioned
   `projects` table in `gallery.db` (the same database that already
   carries the gallery's generations/inputs at schema 15 and the
   `users` table — one store, one migration path). Tables: `projects`
   (id, title, named, updated_at, cover, count, selected, loras, rev,
   owner, deleted-at, extra-json for the forward-compatible fields),
   `project_items` (key, kind, gen_id, input_id, job_id, position,
   extra-json) — or the shape the in-session design decides; the
   merge protocol (the `known`-set three-way) re-implemented as a
   transaction over the tables (the semantics in the current
   `projects.rs` merge() tests are the spec — they move to the DB
   implementation and keep passing).
2. **The wire shape to clients is unchanged**: the `Project` JSON the
   UI sends/receives is byte-identical in shape (the UI stays
   caller-transparent, as D-MU13 made it); the UI test suite and the
   multi-user harness (67) pass against the DB-backed store without
   client changes.
3. **The migration is one-way and proven**: first start with a
   `projects.json` present imports it into the tables (idempotent —
   a second start is a no-op, keyed on the schema version); the file's
   fate after migration is a decision (D-032-1; lean: keep it written
   as a read-only export for one schema version as the rollback path,
   stop after the next bump — the `config.json.bak-*` convention is the
   house pattern). **The box's live file is the test fixture**: the
   owner's account + the 10 battery projects (270 images' worth of
   items) migrate without loss — verified by a before/after diff of
   every project's items/meta/owner, and the battery projects still
   visible in the UI after the cutover.
4. **The D-MU13 semantics keep holding** (the tests move, not the
   behaviour): a named account sees/creates/edits/deletes only its
   own; anything else is the identical 404 (no existence leak); the
   operator/token see everything; a deleted account's projects go with
   it; tombstones prevent resurrection; the single-user case
   (no named accounts) keeps working — and **no longer owes byte-
   identity to any upstream format** (the constraint that shaped the
   side-map is gone; say so in the module comment's replacement).
5. **The config schema advances with it** (16 → 17 if task-031 used
   16, else 15 → 16 — coordinate with task-031's number): the bump
   carries the import state so a rollback to the previous image still
   reads the world it expects (the previous image reads the file; the
   file is still written during the transition — D-032-1).
6. **Full suite green** (the `projects.rs` test count moves to the new
   module; harnesses 67 + 16 green), the image built, the deploy
   window (CPU-side; the data dirs are bind-mounted — the DB file
   gains tables, the live sessions drop and re-login, known), the
   one-step rollback staged (previous image + a copy of the DB and the
   file, as the box convention does for every cutover).

## blockers

- **task-031 first** — the byte-identity constraint (the reason the
  file exists) must actually be gone before this task's premise holds;
  otherwise we are moving data we still owe upstream's format.
- The live box data (the battery projects, the owner's account) — the
  migration is proven on a copy first (the 11-11 lesson applies: never
  let a one-way migration run on live data without a proven rollback).

## decisions

- **D-032-1 — (fill in in-session) the file's fate**: lean = written as
  a read-only export for one schema version (the rollback path), retired
  at the next bump; the DB is the source of truth from the first
  successful import (the file is no longer read after the import, except
  on rollback to the previous image, which predates the DB and reads the
  file).
- **D-032-2 — (fill in in-session) the table layout** (the working
  sketch in the acceptance criteria).
- **D-032-3 — (fill in in-session) where the merge protocol lives**:
  lean = the same home as today (`projects.rs` becomes the DB-backed
  store; the pure `merge()` stays pure and unit-tested against the
  tables' row shape — the merge is a function of values, the store is
  its home).

## progress

- 2026-11-17: task created from the owner's 11-17 message ("we kinda
  hacked that implementation by using a json instead of actual db for
  user projects, on the dated assumption we stick to upstream… right
  now we are invalidating that direction by intentionally moving away
  from upstream's hard path, so maybe we need to do better on this
  too") — which is exactly the recorded trigger: the D-MU13 entry in
  the task-026 file (the "why not the database" answer, 2026-11-13,
  owner's own question) names D-RB as the revisit condition. Queued
  last of the four: it needs both the independence (029) and the
  constraint's retirement (031).
