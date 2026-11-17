# task-029 — independence: the standalone repo + identity

Owner decision 2026-11-17 (follow-through on D-RB, Discussion #52): the fork
becomes its own project. One repo, our name, our releases, full credit to
Kokon. The product is the box service: docker image + browser UI + MCP,
pointing at a gateway of stock engines.

## acceptance criteria

1. **Name decided and recorded** (D-IND1). `kokon-ee` is the default (the
   owner's 11-17 pick). Availability checked 2026-11-17: GitHub user and
   repo free, npm free, crates.io free. Alternative `kokon-box` also checked
   free (parked). The name is cheap to change later (repo rename + compose);
   the only sticky artifact is the ghcr image history, so we lock it now.
2. **The new repo is born with the full history, un-rewritten.**
   `iamskeole/kokon-ee` created as a **non-fork, public** (same
   visibility as the fork; nothing in it is more sensitive than what is
   already public on the fork — the BFL bundle has been public there
   since 11-06); `main` pushed as-is from
   the current tip (no rebase/recommit before the move — author dates and
   SHAs preserved; the squares start from here); the `pr-external-engine-012`
   branch (the #52 proposal record) and the `parked/*` tags come along.
   This clone's `origin` points at the new repo from then on.
3. **The attribution/branding pass is the first new work on the new repo** —
   one unit, never a state where the new name is public without the credit:
   - README: the product blurb becomes ours (the box service: clone → docker
     build → compose → gateway → stock engines; see docs/fresh-box.md), a
     "Derived from Kokon" section (upstream repo link, the #52 thread,
     what this project changed), the "maintained fork" wording gone.
   - NOTICE: keeps Gabriele Lozupone's copyright for the derived parts,
     adds ours for new parts (D-IND5), AGPL-3.0-only stays (the code is a
     derivative — no choice).
   - Brand: our wordmark replaces his logo artwork (his artwork is not
     AGPL-licensed; we don't put his face on a renamed product). Default:
     a plain "Kokon EE" text wordmark in the existing Inter; an icon is a
     later nicety, not a blocker (D-IND6).
   - MCP identity: package name, `mcpName` → `io.github.iamskeole/kokon-ee`,
     author, description ("against the Kokon EE service" instead of
     "requires the desktop app"), the glama.json maintainer, the registry
     badges (removed until re-listed under the new identity; re-listing is
     an owner action, not a blocker).
   - Repo description field + branch list tidied (as the 11-06 tidy did).
4. **Host deploy renamed to match**: the image line becomes
   `ghcr.io/iamskeole/kokon-ee:<tag>` (local build; a ghcr push needs the
   owner's token as always — not a blocker), `container_name` → `kokon-ee`,
   the deploy dir `backends/../frontends/kokon` → `frontends/kokon-ee`
   (bind-mounted data/config/gallery move with it; old compose + image kept
   as the one-step rollback). The container restart kills open web sessions
   (known foot-gun) — the owner re-logs in. No GPU impact: the service
   image is CPU-side; the engines and vllm are untouched.
5. **The old fork is archived, not deleted**: `iamskeole/kokon` archived on
   GitHub with a README note pointing at the new repo (the historical
   record of the pre-independence line, including the #52 proposal branch
   as it was pushed).
6. **The author hears it from us, not from a diff**: a short reply posted in
   Discussion #52 (the owner sends it from iamskeole; the draft is in
   `## decisions` below) — the fork is becoming its own project under the
   name X, full attribution in the repo, thanks for the code and the kind
   reply.
7. **The tracker formalizes the sync marker**: STATUS records
   `last-synced-upstream = f9b6971 (0.12.0)` (what task-030 will advance);
   the "oldest todo = current" pointer in STATUS names task-029 as current
   and 023/025/027 as still parked on the owner's trigger.
8. Box check after the cutover: service up (ping + `/status` with the
   service token), the owner's login works, the gallery/projects intact
   (the data dirs moved, not rewritten).

## blockers

- ~~Owner: confirm the name~~ — **resolved 2026-11-17: `kokon-ee`
  confirmed** (the parked `kokon-box` alternative is retired).
- **Owner: sends the #52 message** (draft in decisions; one short paragraph,
  their words ideally).
- ghcr push (only if we want the image publicly pullable): the owner's
  token with packages scope (the 11-11 note). Not a blocker for the move —
  the deployment is a local build.
- (Minor) the service container name is referenced by the box's compose
  backups and by any operator habit (`docker logs kokon`) — cosmetic;
  documented in the deploy README.

## decisions

- **D-IND1 — Name = `kokon-ee` — CONFIRMED 2026-11-17** (final). Readable as
  "Kokon, external-engine": it signals exactly the difference to the
  audience that matters (people with a box + gateway), keeps the lineage in
  the name (which is also the attribution), and is free in every namespace
  checked. Not a brand pun (unlike the earlier `kokoff` candidate, which
  collides with a person's surname and reads as riffing on a brand whose
  owner just declined us). Changing it later is one repo rename + one
  compose edit; the ghcr image history is the only artifact that keeps the
  old name.
- **D-IND2 — Plain history push, no rewrite before the move.** A rebase or
  recommit would reset dates (the squares) and fork the SHA set; both are
  unnecessary — the move is a copy, and the 0.12.1 rebase happens *after*,
  on the new repo (task-030), where a force-push of our own `main` is
  normal.
- **D-IND3 — The old fork stays, archived.** Deletion would burn the
  provenance (and the #52 proposal as pushed). Archived = read-only, out of
  search, unambiguably the old home.
- **D-IND4 — Attribution and rename land as one unit on the new repo,
  before any other work.** The new identity never appears without the
  credit; the git log already carries his authorship on every derived
  commit, which the README states.
- **D-IND5 — NOTICE wording (draft, owner may edit):** "Kokon EE —
  Copyright 2026 Gabriele Lozupone (the parts derived from Kokon) and 2026
  iamskeole (the parts this project adds)." The legal name behind
  `iamskeole` is the owner's choice; default: the GitHub handle, matching
  the commit sign-offs.
- **D-IND6 — The wordmark now, the icon later.** A text wordmark in the
  bundled Inter needs no design and no artwork questions; an icon can come
  in any later pass without touching this task.
- **D-IND7 — The #52 reply (draft for the owner to send):**
  "Thanks again for the thoughtful reply. In the spirit of your
  'right home for that setup': the fork is becoming its own project —
  `kokon-ee`, a standalone repo that keeps the full history and credits
  Kokon properly in the repo (the external-engine mode, the multi-user
  service, the gateway-based setup). Nothing here changes what you built;
  this is just the fork growing up. I'll keep tracking your releases —
  the UI is still the best part we take from you."

## progress

- 2026-11-17: task created (owner's 11-17 message: do the fork thing now,
  name kokon-ee, ensure the build survived the weekend disk cleanup, use
  0.12.1 as the in-vivo independence exercise). Orientation done: upstream
  0.12.1 = 10 commits (`bff35ca`), all read (the three interactions are
  task-030's); the D-MU13 recorded trigger (the projects JSON→DB question)
  is documented as task-032. Box state: service image
  `ghcr.io/iamskeole/kokon:multiuser-7c3cb37` == the code tip (the 5 commits
  after it are tracker-only); the weekend cleanup deleted `target/` (the
  toolchain and node_modules survived) — full cargo rebuild + all suites
  re-run in session 11-17, results recorded here as they land.
- 2026-11-17 (later): **owner confirmed the name (`kokon-ee`) and the Tauri
  shell cut (D-031-1, recorded in task-031).** All owner inputs for this
  task are now in except sending the #52 reply (D-IND7 — do it any time
  around the move, before or right after; the author has no automatic
  notification of a rename). Ready for the next session: move to `doing/`
- 2026-11-17 (the move session) — all ACs met:
  - **AC2** `iamskeole/kokon-ee` created (non-fork, public, default
    `main`; this gh version has no `--empty` flag) and the full history
    pushed as-is (all branches and tags came along); this clone's `origin`
    repointed. **AC7** pointer flipped (below).
  - **AC3** the identity pass landed as four signed commits, before
    anything else (D-IND4): the MCP identity (`kokon-ee-mcp`,
    `io.github.iamskeole/kokon-ee`, service-first README, registry badges
    out until re-listed), the README product blurb + "Derived from Kokon"
    (upstream link, #52, what changed, the clone/build/compose release
    line), NOTICE dual copyright (D-IND5) + the Kokon EE text wordmark
    replacing the upstream logo artwork (D-IND6; his mark/icons stay in
    the tree as inherited artwork, flagged in the brand README), and the
    AGENTS.md mission wording + deploy docs on the new coordinates.
  - **AC4** host deploy renamed: `frontends/kokon` →
    `frontends/kokon-ee` (compose updated; service and container
    `kokon-ee`; image **re-tagged** `ghcr.io/iamskeole/kokon-ee:multiuser-7c3cb37`
    from the old tag — no rebuild, the 5 commits after 7c3cb37 are
    tracker-only; old compose kept as `compose.yaml.bak-kokon`, old tag
    still present = the one-step rollback). The **source clone renamed
    too** (`/home/p/code/iamskeole/kokon-ee` — the owner's 11-17 question):
    zero references found in the gateway deploy, cron or dotfiles; the
    deploy README and the docs' dev-home line follow. **AC8** box check
    after the recreate: ping 200, `/status` ok with the service token,
    358 gallery files + `gallery.db` + all data/config JSON byte-identical
    to the pre-move md5s, login door 403 on a wrong password (owner
    re-login = the known foot-gun, noted in the deploy README).
  - **AC5** old fork archived: banner commit `15fddcf` pushed to
    `iamskeole/kokon` main, description set, `archived=true` via API PATCH
    (this gh version has no `--archived` flag).
  - **Tests before the commits**: cargo 406+20, npm 63, mcp **35+3sk, 0
    failed** (tairi 2). The two protocol tests needed the release
    `kokon-service` rebuilt after the weekend `target/` wipe. **Box
    quirk, recorded**: linuxbrew's openssl is 4.1.0 (10-23), so a
    host-built service needs `LD_LIBRARY_PATH=/home/linuxbrew/.linuxbrew/opt/openssl/lib`
    (the host has no libssl-dev and sudo is owner-only; the 11-06 binary
    worked because it linked the pre-upgrade 3.x whose SONAME matches the
    system `libssl3t64`). Permanent fix = `apt install libssl-dev` — **owner ran it 2026-11-17, verified**: a clean-env `cargo check -p qls-service` (no `OPENSSL_DIR`/`PKG_CONFIG_PATH`/`LD_LIBRARY_PATH`) now resolves the system openssl 3.5.5, so no env-var dance on this box anymore. The Docker image build was never affected (it apt-installs its own).
  - **AC6** the D-IND7 draft went to the owner in the session report;
    sending it is the only remaining owner action.
  - Housekeeping: the session's pi cwd was pinned to the old clone path —
    a symlink `/home/p/code/iamskeole/kokon -> kokon-ee` keeps the running
    session alive; drop it when the session restarts.

## done

2026-11-17. Repo born, identity pass in, fork archived, box renamed and
checked, tests green.
