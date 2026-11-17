# task-034 — the failure toast is for news, not for history

Small fix, split out of task-031 on the owner's call (2026-11-17 17:50):
031 is the big chaff cut and stays focused on its own body of work;
this is a few-line UI bug that bit the owner live the same afternoon.

## the incident

The owner's UI kept toasting **"The inference server did not report its
capabilities. (HTTP 500 Internal Server Error)"** — no generation
running, gateway journal silent. Task-030's post-close note had called
it a "stale failed job re-rendered"; the recurrence showed the actual
mechanism:

- `store.ts` `onJob()` (and `onReviewJob()`) toasted on **every
  observation** of a failed job — `if (job.status === "failed" &&
  job.error) toastError(...)` — with no notion of news.
- `startEvents()`' reconnect catch-up runs `api.jobs().then(js =>
  js.forEach(onJob))` — **every** stored job re-flows through `onJob`
  on each (re)connect and each initial load.
- So the two w030 run-5/6 jobs that failed at 12:08/12:20 (before the
  14:50 engine-file restore) re-toasted on every session and project
  view — for hours, independently of anything on the GPU.
- Jobs are **in-memory** (no jobs file in `data/`; `gallery.db` /
  `projects.json` hold images and projects), so a service restart
  clears them — that was the stopgap applied 17:29 (verified: vLLM /
  registry / config untouched, startup re-restored the remembered
  turbo caps from `external-caps.json` with zero gateway traffic).
- The sibling lines in the same `onJob` already used the transition
  idiom for two other behaviours ("only for a job that just failed,
  not old ones listed again after reconnecting" — the missing-model
  guard; the fresh-results hatch) — the toast line simply never got
  it.

## the fix (commit 508182b)

- New exported predicate `freshFailure(prev, job)` in `store.ts`:
  a failure is toasty when `prev?.status !== "failed"` (the client
  last saw it live — or never saw it) **and** `finished_at` is within
  the last 60 s (so a first sight of a job that ran while the tab was
  closed still alarms once, while first sight of an hours-old
  failure is history). The 60-s window mirrors the missing-model
  guard's, whose alarm is likewise only actionable while fresh.
- Both toast sites (`onJob`, `onReviewJob`) call it; the error
  presence stays checked at the call site (it narrows the type for
  `ApiError`).
- `store.test.ts` (new): the w030 matrix — live transition toasts;
  fresh first sight toasts; stale first sight (the fossils) and
  re-sights (the catch-up spam) do not; non-failures and missing
  `finished_at` do not. UI suite 239 → 245, tsc clean.

## deployment

- Image `ghcr.io/iamskeole/kokon-ee:multiuser-508182b` built on the
  box (the build host), box `compose.yaml` re-pointed, `up -d` —
  seconds of service downtime, no GPU involvement (not a window).
- Verified after: 0.12.1 up, `state: stopped` with the remembered
  turbo caps (21 samplers, enhancer visible, `error: null` — the
  designed known-but-not-loaded indicator state), 0 jobs, the
  owner's 16:15 config (`53c2a283…`) and the registry (`471ba26c…`)
  byte-identical, vLLM the same process (23868 MiB, `/health` 200).
- **Ops note:** the `docker push` to ghcr.io was **denied** from this
  box (stale GH credentials — the short-lived token from the
  task-030 build has lapsed). It did not block this deploy (local
  image on the build host), but the **stable box** (the prod UI,
  which pulls from ghcr) cannot get this fix — or any future image —
  until the credentials are refreshed (`docker login ghcr.io` with a
  current token). Flagged to the owner 2026-11-17.

## done

2026-11-17: fixed, tested (245), built, deployed, verified. The
owner's toasts stop on the next UI load (no failed jobs left in the
store, and any future stale failure will stay in the project view
without alarming).
