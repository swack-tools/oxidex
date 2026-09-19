# Docs site: build and deploy

This site is [VitePress](https://vitepress.dev/). Its sources are in `docs/`
and its configuration (nav, sidebar, footer) is in
`docs/.vitepress/config.mts`. It is published at **https://oxidex.net** from
`main` only. Nothing on `refactor/tag-machinery` reaches oxidex.net until the
maintainer merges it to `main`.

## Build it locally

```bash
cd docs
npm ci
npm run docs:dev      # live-reloading dev server
npm run docs:build    # the production build CI runs; fails on any dead link
npm run docs:preview  # serve the production build
```

`npm run docs:build` fails on any dead internal link. Two route prefixes are
exempt, and the exemptions are listed in `ignoreDeadLinks` in `config.mts`:

- `/benchmarks/`: the Criterion reports are copied into the built site at
  deploy time. They are not part of the source tree.
- `/status/`: the generated project status page arrives with #837
  (`tools/docs/render_status.py`). Remove this exemption once it lands.

`docs/reference/comparison/` is gitignored. It holds the per-format
comparison against the pinned ExifTool, which the deploy workflow generates
on every run. On a fresh clone, `npm run docs:build` first runs
`docs/scripts/ensure-comparison-stub.mjs`. That script writes a clearly
labelled placeholder page, so every link resolves. For the real tables, run
`just compare-exiftool-full-update`. It needs a release build, the pinned
ExifTool and a perl with `Archive::Zip`, and it is slow.

## Preview it the way the deploy builds it

`tools/docs-local-deploy.sh` (#835) mirrors the `publish` job of
`deploy-docs.yml`. It builds the site into a temporary directory and serves
it on localhost. By default it builds from a `git archive` snapshot of a ref,
so your checkout is not touched.

```bash
tools/docs-local-deploy.sh                          # origin/refactor/tag-machinery, placeholder report
tools/docs-local-deploy.sh --ref origin/main        # what main would publish
tools/docs-local-deploy.sh --worktree --no-open --port 4180   # this checkout, uncommitted edits included
tools/docs-local-deploy.sh --bench auto             # + the latest CI benchmark-results artifact
tools/docs-local-deploy.sh --full-report            # + the real ExifTool comparison (slow)
```

`--keep` keeps the temporary directory. Environment variables pass through to
the VitePress build.

Open the served site in a browser, not `index.html` from disk. The site uses
clean URLs and absolute asset paths, so a `file://` URL breaks every link and
style.

## Verify release documentation locally

For a release, use `oxidex-release-documentation` against the frozen full
candidate SHA. Its production-equivalent local deploy must include the real
comparison report and the selected benchmark artifact, then reconcile the
committed, generated, and rendered inventories. Use browser automation to
navigate every reconciled inventory route, including pages absent from
navigation, and capture console, page, request, network, and HTTP failures.
Check every referenced asset and fragment. Separately, retain responsive
light/dark desktop/mobile screenshots for a representative screenshot matrix
and record human visual review of those screenshots; this matrix complements
the exhaustive browser navigation rather than replacing it.

That exact-candidate local evidence can verify the documentation before it is
published. Live Pages validation is optional operational confirmation. If the
final `main` tree differs, rerun the same local audit against the exact merge
commit before tag authorization.

## Checks on pull requests

`docs-build.yml` runs on every PR that touches `docs/`. It does a cold
`npm ci && npm run docs:build`, confirms that the comparison report is not
committed, and checks that the placeholder page renders.

## Deployment: `deploy-docs.yml`

The workflow runs on a push to `main` that touches `docs/**`, `benches/**`,
`src/parsers/**`, `src/bin/tag-comparison/**` or the workflow itself. It can
also be started manually. A newer run cancels an older one. It has two jobs:

1. **`comparison-report`** runs with `contents: read` only. It resolves the
   pin from `.exiftool-version` and restores the cached pinned ExifTool and
   sample corpus. After checking that the cache reports the pinned version,
   it runs `just compare-exiftool-full-update`, then uploads
   `docs/reference/comparison/` as an artifact.
2. **`publish`** holds `pages: write` and `id-token: write`, and runs no
   project code:
   - It downloads the comparison report.
   - It finds a `ci.yml` run that carries a `benchmark-results` artifact
     (Criterion output). It tries the same commit first, then recent `main`
     runs.
   - If that artifact is present, it stamps the **Date** and **Commit** lines
     on the [performance page](/performance/) with the commit the numbers
     came from, which is not necessarily HEAD.
   - It builds the site, copies the Criterion reports to `/benchmarks/`, and
     deploys with `actions/deploy-pages`.

GitHub Pages for `swack-tools/oxidex` is set to **GitHub Actions** as the
source (`build_type: workflow`). Release verification must recheck that API
setting and audit workflow syntax/tests, triggers and path filters,
permissions, generated-report and benchmark provenance, Pages artifact
handoff, deployment environment, concurrency, custom domain, base path, and
HTTPS. A `gh-pages` branch update is not deployment proof in workflow mode;
the legacy branch-writing job in `release.yml` must not be credited as a Pages
deployment without independent evidence. The custom domain **oxidex.net** is
set in the repository's Pages settings, not by a `CNAME` file, and HTTPS is
enforced. VitePress builds with `base: '/'`.

## Generated content

Some pages are written by tools. Never edit them by hand. Rerun the generator
and commit its output.

| Page | Generator |
| --- | --- |
| `reference/comparison/` (not committed) | `just compare-exiftool-full-update`, at deploy time |
| `reference/tag-coverage-analysis.md` | `scripts/generate_tag_coverage.py` (`just docs-coverage`; `update-coverage-docs.yml` uploads a patch artifact) |
| `reference/catalog-baseline.md` | `tools/exiftool-tables/catalog_snapshot.py --report` |
| `reference/catalog-hydrated-join.md` | `tools/exiftool-tables/join_catalog_hydrated.py` |
| `reference/catalog-hydrated-observed.md`, `reference/catalog-corpus-observed.md` | `tools/exiftool-tables/catalog_observed_snapshot.py --report` |
| `reference/jpeg-tag-support.md`, `reference/jpeg-tag-matrix.md` | the `jpeg-tag-matrix` binary (`jpeg-tag-matrix.yml`) |
| `tag-domains/*.md` (except `index.md`) | `just docs-generate-tags` (`oxidex-tags/examples/render_domain.rs`) |
| Performance **Date**/**Commit** lines | `deploy-docs.yml`, at deploy time |

A handful of numbers in hand-written pages are kept current by
`scripts/sync_tag_stats.py`: the tag-definition count and the conformance
score on the home page, for example. CI runs it with `--check`. Every rule
must match exactly once, so if you reword one of those sentences, update the
rule in the script in the same change.

## A preview channel: designed, not deployed

::: warning Not live
This section describes draft PR #836. Nothing it describes is deployed, and
the repository it publishes to does not exist yet. It is recorded here as a
documented option for the maintainer.
:::

oxidex.net shows `main`. Reviewing the refactor branch's docs needs either a
local build (above) or a second, clearly labelled site. #836 designs that
second site:

- **Where it goes.** A separate public repository,
  `swack-tools/oxidex-next`, serves the preview from its own `gh-pages`
  branch at `https://swack-tools.github.io/oxidex-next/`. No DNS change is
  needed. The design does not use `oxidex.net/next/`: this repository's
  Pages deploy replaces the whole site on every run, so the preview workflow
  would have to rebuild and republish the stable site each time. It would
  also need the Pages credential that can replace oxidex.net. A separate
  repository gives the preview workflow no path to the live site at all.
- **How it deploys.** A new `deploy-docs-preview.yml` runs on each push to
  `refactor/tag-machinery` that touches `docs/**`, and can also be started
  manually. Its build job has `contents: read`. Its deploy job has no token
  permissions and runs in an environment named `docs-preview`. The deploy
  job force-pushes one orphan commit to `oxidex-next`'s `gh-pages` branch
  over SSH, with a deploy key and GitHub's host keys pinned. If the key is
  missing, the build still runs, and the deploy reports "not configured"
  and skips.
- **What readers see.** `config.mts` becomes environment-driven
  (`DOCS_CHANNEL`, `DOCS_BASE`, `DOCS_STABLE_URL`, `DOCS_PREVIEW_URL`,
  `DOCS_PREVIEW_SHA`):
  - On both channels, the version dropdown gains a **Versions** group that
    links `v1.2.1 (stable)` and `branch: refactor/tag-machinery (preview)`.
  - The preview channel labels the dropdown with the branch name. It shows a
    banner across the top of every page: "Development preview of
    refactor/tag-machinery at `<sha>`, not a release".
  - The preview adds `noindex, nofollow`, points "Edit this page" at the
    branch, and sends `/benchmarks/` links to the stable site.
  - `docs-build.yml` becomes a two-entry matrix that builds both the stable
    (`/`) and the preview (`/oxidex-next/`) base.
  - A link checker (`npm run docs:check-links`) catches root-absolute links
    that would break under a non-root base.

**Settings it would need**, all of them maintainer-only:

1. Create the public repository `swack-tools/oxidex-next`.
2. Generate a deploy key (`ssh-keygen -t ed25519 -N '' -f oxidex-next-deploy`)
   and add its public half to `oxidex-next` with **write access**.
3. In `swack-tools/oxidex`, create an environment named `docs-preview`,
   restricted to the `refactor/tag-machinery` branch. Store the private half
   of the key in it as the secret `DOCS_PREVIEW_DEPLOY_KEY`.
4. After the first successful push creates the branch, set `oxidex-next`'s
   Pages source to **Deploy from a branch**: `gh-pages`, `/ (root)`.
5. Optional: serve it as `next.oxidex.net`. Add a DNS `CNAME` record for
   `next` pointing at `swack-tools.github.io`, set that custom domain on
   `oxidex-next`, and then change `DOCS_BASE` to `/` and update
   `DOCS_PREVIEW_URL` in the workflow and in `config.mts`.
