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
- **D-033-2 (2026-11-23):** the publisher is the repository's CI (its
  per-run GITHUB_TOKEN, `permissions: packages: write`), **not** a PAT on
  any box — no ghcr credential exists on any machine, so a recipient box
  just `docker pull`s (the package is public, as is the repo). Supersedes
  the plan's item 1 (a box-held token).

## Done (2026-11-23, night — owner: "can we flag 33 as done?")

The trigger (030/031/032 done) had fired with 032's close; the owner gave
the word tonight. The resolution deviated from the plan in one way, for
the better: **no token lives on any machine at all** (D-033-2). The box's
gh CLI token never carried `packages:write` (verified: its ghcr token
request gets 403 for push, 200 for pull) — which is why every local push
failed; the Actions workflow's own token can, and run `45222545666` is
the proof (green, and it pushed the image).

- **Item 1 (a token):** retired by the design — the publisher is CI.
- **Item 2 (tag convention):** `multiuser-<short sha>` + `latest` on every
  push to `main`; `workflow_dispatch` takes any `ref`.
- **Item 3 (the push):** proven; from then on the boxes pull.
- **Item 4 (visibility):** public package — the repo is public, the same
  exposure class this file anticipated ("compiled source"). Verified on
  the wire: an anonymous token + manifest fetch returns 200, and the dev
  box pulls with zero docker credentials configured.
- **Item 5 (docs):** `docs/fresh-box.md` §4 (pull is the primary path;
  build-from-source stays the verifiable route; the compose example
  corrected to the live shape — bind mounts, host network, no GPU
  reservation, the token in `service.json`), the README quickstart, and
  `docker/compose.yaml` + `docs/docker.md` (both still described the
  *upstream bundled product* — the chaff cut's last remnants; they now
  describe the service box).

**First real consumer:** the stable box (atlas) — task-027's prod-box
half, whose trigger fired the same night (`https://imagine.unveilx.com`
behind atlas's nginx, the `chat.unveilx.com` model).
