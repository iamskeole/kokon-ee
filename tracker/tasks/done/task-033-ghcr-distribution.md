# Task-033 (PARKED — owner-triggered) — distribution via ghcr (pushed service image)

**Created:** 2026-11-17 — the owner's call at the close of task-029:
stick to the current distribution path (clone the repo → `docker build` →
`docker compose up -d`) **until the current work is done** (030 the 0.12.1
rebase, 031 the chaff cut, 032 projects → DB), and then move distribution
to ghcr. **Do not start this task before the trigger** — the image content
should be the phase's *final* form before it is pushed, or we'd be shipping
artifacts we are about to change.

## Trigger (when this stops being parked)

The current phase is done (030/031/032) and the owner says "let's ship
the image" — or the owner decides earlier that a second machine/person
needs the one-command path (pull instead of build).

## What the task contains

1. **A token**: a GitHub personal access token with `packages: write`
   (fine-grained, scoped to this repo, is the least-key shape), then
   `echo $TOKEN | docker login ghcr.io -u iamskeole --password-stdin`.
   Owner-owned, like every other credential on the box.
2. **A tagging convention**: the local tag already has the right shape —
   `ghcr.io/iamskeole/kokon-ee:<mode>-<commit>` (the box runs
   `multiuser-7c3cb37` under exactly this name, so nothing moves, only
   the push step is new). The open decision: what gets a push and when
   (e.g. once per phase, the tag = the phase's tip commit; a version
   name the owner picks is also fine).
3. **The push itself**: `docker push` — and from then on a recipient's
   compose file points at the same image name with no build step; the
   one-line flip is noted in the docs.
4. **Visibility, decided at that moment**: a public repo's package is
   public by default (anyone who knows the name can pull — same exposure
   class as the repo's source, but compiled); a private package keeps
   pulls behind auth. Free-tier details get checked then, not now.
5. **Docs**: the README "how to get it" and `docs/fresh-box.md` gain the
   pull path alongside the build path (both stay true: build-from-source
   remains the verifiable route).

## decisions

- **D-033-1 (owner, 2026-11-17):** distribution stays
  clone → build → compose until the current phase (030–032) is done;
  ghcr is the intended distribution mechanism after that ("the intended
  distribution workflow works for now"; the owner wants the moving parts
  understood before adopting them). This file is the follow-through.
