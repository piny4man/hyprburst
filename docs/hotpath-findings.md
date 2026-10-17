# Hotpath findings — 2026-10-17

Baseline for [NUKE-109](https://linear.app/tiendacables/issue/NUKE-109),
recorded with the harness in [Profiling with hotpath](hotpath-profiling.md).
Fixes are follow-ups; nothing here changes launcher behavior.

## Environment

- AMD Ryzen 7 8845HS (8 cores/16 threads), 60 GiB RAM, Linux 6.18.57-1-lts, on
  AC power.
- Hyprland 0.56.2, one 2880×1800 panel at 120 Hz and scale 1.5.
- hyprburst 0.8.1 on `feat/hotpath-profiling` (based on `95fd39d`), release
  profile; hotpath reports from a separate `--features hotpath-alloc` build.
- 107 discovered apps; `[font] path` set to a Nerd Font, 2875 fonts installed.
- `powerprofilesctl` `performance` (the machine's setting), then `balanced`,
  then restored to `performance`.

Runs, for each profile:

- 30 cold and 30 warm `--measure` opens of each frontend with
  `HYPRBURST_CHILD=1 HYPRBURST_PERF=1` (cold: 2 s idle before each open; warm:
  back to back). The page cache was not dropped (no root), so "cold" is a
  clocked-down CPU, not a cold disk.
- Live sessions of each frontend, typed by hand (`fi`, two `Backspace`, `Down`,
  `Down`, `Up` with ~0.5–1 s pauses), until each metric had at least 30
  samples.
- `query-cost --live 40` tight and with 200 ms idle between steps.
- One hotpath open report per frontend, and one TUI-child report driven over a
  PTY.

## Targets

The figure of record for first presentation is `first-content-present`: the
first frame the user can read. For Rio, `first-present` (what `--measure`
reports) is an empty panel ~30 ms earlier; it is listed for comparison.

### First presentation, p95 (ms)

| Open | Frontend | Performance | Balanced | Change | ≤ 10% |
| --- | --- | ---: | ---: | ---: | :---: |
| cold | native | 86.0 | 94.0 | +9.3% | pass |
| cold | rio | 114.5 | 123.4 | +7.7% | pass |
| warm | native | 91.1 | 103.9 | +14.1% | **fail** |
| warm | rio | 122.5 | 130.2 | +6.3% | pass |
| cold | rio, blank frame | 79.3 | 89.2 | +12.5% | (fail) |
| warm | rio, blank frame | 89.2 | 98.7 | +10.6% | (fail) |

Medians (performance → balanced): cold native 80.6 → 86.9, cold rio 109.3 →
118.6, warm native 87.2 → 92.1, warm rio 116.1 → 123.1. Warm opens are
consistently ~7 ms slower than cold ones; back-to-back launches compete with the
previous process's teardown.

### Query and selection, input to present (ms)

| Metric | Frontend | Profile | n | Median | p95 | Max | p95 < 50 ms |
| --- | --- | --- | ---: | ---: | ---: | ---: | :---: |
| query | rio | performance | 100 | 0.83 | 1.09 | 642.9 | pass |
| query | rio | balanced | 62 | 1.51 | 1.93 | 189.9 | pass |
| query | native | performance | 29 | 0.30 | 0.57 | 1.7 | pass |
| query | native | balanced | 68 | 0.59 | 0.90 | 1.6 | pass |
| selection | rio | performance | 56 | 0.75 | 0.95 | 550.8 | pass |
| selection | rio | balanced | 36 | 1.46 | 1.93 | 124.7 | pass |
| selection | native | performance | 41 | 0.35 | 0.50 | 0.5 | pass |
| selection | native | balanced | 64 | 0.52 | 0.61 | 0.7 | pass |

Balanced roughly doubles input latency, but it stays 25× under budget. The Rio
maxima are probe artifacts (finding 5). Native in performance has 29 query
samples, one short of 30; the balanced runs, which the target is judged on,
have at least 30 everywhere.

Offscreen (`query-cost --live`, 107 apps), median / p95: query 0.038 / 0.065
ms and selection 0.021 / 0.035 ms in performance; 0.071 / 0.110 and 0.043 /
0.052 ms in balanced, with 200 ms idle between steps. In a tight loop both
profiles reach 0.012 / 0.013 ms. Filtering and rendering are not where input
latency goes.

## Where an open goes

Hotpath, one cold `--measure` open (performance; balanced within ±3 ms):

| Function | Native | Rio parent |
| --- | ---: | ---: |
| `resolve_fonts` | 43.7 ms | 41.4 ms |
| `GlState::new` | 29.4 ms | 30.8 ms |
| `CellRenderer::new` | 4.4 ms | 4.3 ms |
| `LauncherCore::new` (incl. `discover_apps` 3.9 ms) | 4.1 ms | in child |
| `Session::spawn` | — | 1.6 ms |
| `History::open`, `Config::load`, `load_scores` | < 0.3 ms | in child |
| first `render_core` + `build_cells` + `draw` | 0.4 ms | 0.02 ms |

Rio timeline (cold, performance, wall clock from `main`): the window presents
an empty frame at ~77 ms and spawns the child at ~73 ms. The child is ready
~4 ms after it starts (`LauncherCore::new`), but its first draw comes ~16 ms
later still, and the parent presents content at ~107 ms.

## Ranked findings

Ranked by expected impact on what the user waits for.

### 1. `resolve_fonts` spawns two fontconfig processes on every open (~42 ms)

**Fixed in [NUKE-111](https://linear.app/tiendacables/issue/NUKE-111); see
[After NUKE-111](#after-nuke-111--2026-10-17).**

`resolve_fonts` takes 41–47 ms on every open in both frontends, about half of
native first presentation. Even with `[font] path` set, it runs `fc-match
monospace` (~9.5 ms measured standalone) and, because that match (Noto Sans
Mono) is not a Nerd Font, an `fc-list` scan of all 2875 installed fonts
(~21 ms) to find an icon fallback. The rest is reading and parsing the font
files.

Proposed fix: cache the resolved text and icon font paths (keyed by config and
the fontconfig cache mtime) under `$XDG_CACHE_HOME`, and skip the scan when
`[font] path` is already a Nerd Font. Expected impact: ~30 ms off every open,
in both profiles.

### 2. Fonts and GL context are set up one after the other (~29 ms)

**Fixed in [NUKE-112](https://linear.app/tiendacables/issue/NUKE-112), widened
to all pre-GL CPU work (fonts and the native `LauncherCore`); see
[After NUKE-112](#after-nuke-112--2026-10-17).**

`GlState::new` (EGL display, config, context, surface) takes ~29 ms, and runs
after `resolve_fonts`. Neither needs the other until the cell renderer is
built.

Proposed fix: resolve and load fonts on a thread while the window and GL
context are created. Expected impact: up to the shorter of the two (~29 ms
today, less once finding 1 lands) off first presentation.

### 3. Rio shows an empty panel for ~30 ms; the child waits a frame before drawing

**Fixed in [NUKE-113](https://linear.app/tiendacables/issue/NUKE-113); see
[After NUKE-113](#after-nuke-113--2026-10-17).**

The Rio parent presents its first frame before the PTY child has drawn
anything, and only spawns the child after GL setup. The child then calls
`poll_batch` (a 16 ms input wait) before its forced first draw. Together the
user sees an empty panel from ~77 ms to ~107 ms.

Proposed fix: draw once before the first input poll in `run_tui`; spawn the
child before font and GL setup, so its start overlaps them; skip presenting
until the grid has content (or keep it but count it as such). Expected impact:
~16 ms from the draw order alone, and up to ~30 ms with the spawn moved,
bringing Rio content close to native.

### 4. Warm native opens miss the balanced target (+14.1% at p95)

**Resolved in [NUKE-115](https://linear.app/tiendacables/issue/NUKE-115) by
re-measuring after NUKE-111..114; see
[After NUKE-115](#after-nuke-115--2026-10-17).**

Back-to-back native opens in balanced have a long tail (p95 103.9 ms, max
110 ms vs 91–92 ms in performance). The cold case and both Rio cases pass. The
open path is dominated by findings 1 and 2, which are process spawns and driver
work sensitive to clock ramp-up; removing them should shrink both the absolute
cost and the profile gap. Re-measure after 1 and 2 before treating this
separately.

### 5. The probe folds no-op keys into the next change on Rio

**Fixed in [NUKE-114](https://linear.app/tiendacables/issue/NUKE-114); see
[After NUKE-114](#after-nuke-114--2026-10-17).**

7 of 254 Rio input samples are 110–643 ms, each about one pause between key
presses. A key that changes nothing on screen (e.g. `Up` on the first row,
`Backspace` on an empty query) produces no damage, so the pending sample is
answered by the next key's frame. Native marks every keyed frame as changed, so
it never folds. This is a measurement artifact, not latency; p95 is unaffected.

Proposed fix: have the Rio parent time out a pending sample after one refresh
with no damage, or have the native path use the same damage rule, so the two
frontends count keys the same way.

### Fixed here: `--measure` delayed the Rio child's first paint by ~70 ms

`report()` runs the footprint probe, which spawns `cargo tree` for the
dependency count, on the event loop thread right after the first present. For
Rio this blocked the PTY wake: content appeared at ~185 ms instead of ~107 ms.
With `HYPRBURST_PERF` set, `--measure` now reports after content has been
presented; the reported cold-start value is unchanged.

## After NUKE-111 — 2026-10-17

Branch `perf/font-resolution` (on `9554c90`). Same machine, procedure and
sample counts as the baseline (30 cold + 30 warm opens per frontend and
profile), with this machine's config (`[font] path` set to 0xProto Nerd Font
Mono).

`resolve_fonts` no longer runs fontconfig when the configured (or
`HYPRBURST_FONT`) font already has Nerd Font icons; otherwise the fontconfig
answer is cached in `$XDG_CACHE_HOME/hyprburst/fonts` and looked up again only
when the font inputs or the fontconfig cache directories' mtimes change, or a
cached font is gone.

`resolve_fonts` per open (hotpath): 39–43 ms before; 1.1–1.4 ms after on the
skip path, 1.4–2.2 ms on a cache hit without `[font] path`. An uncached lookup
still costs 30–40 ms and happens once after fonts change.

### First presentation, p95 (ms)

| Open | Frontend | Performance | Δ baseline | Balanced | Δ baseline | Balanced vs perf | ≤ 10% |
| --- | --- | ---: | ---: | ---: | ---: | ---: | :---: |
| cold | native | 42.9 | −43.1 | 46.9 | −47.0 | +9.4% | pass |
| cold | rio | 74.9 | −39.6 | 77.8 | −45.5 | +3.9% | pass |
| warm | native | 49.4 | −41.7 | 52.6 | −51.3 | +6.3% | pass |
| warm | rio | 80.6 | −41.9 | 80.1 | −50.2 | −0.6% | pass |
| cold | rio, blank frame | 39.9 | −39.4 | 43.0 | −46.2 | +7.8% | (pass) |
| warm | rio, blank frame | 45.8 | −43.4 | 44.5 | −54.2 | −2.7% | (pass) |

Medians (performance → balanced): cold native 39.6 → 43.5, cold rio 70.0 →
74.3, warm native 42.0 → 46.4, warm rio 70.3 → 73.9.

NUKE-111's done-when holds: `resolve_fonts` is under 15 ms with a warm cache,
and native first-content p95 dropped by 41–51 ms (≥ 25 ms required) in both
profiles. Every first-presentation target now passes, including warm native
(finding 4, tracked in NUKE-115), although cold native at +9.4% has little
margin.

What remains, in order:

- `GlState::new` is now the largest open phase: 28–30 ms in performance, 42 ms
  in one balanced report (NUKE-112).
- Rio content still arrives 31–35 ms after its blank frame (NUKE-113).

## After NUKE-112 — 2026-10-17

Branch `perf/overlap-startup` (on `ed14bcb`, which includes NUKE-111). Fonts
(both frontends) and the native `LauncherCore` (app discovery, history,
scores) are now built on worker threads, `hyprburst-fonts` and
`hyprburst-core`, while the event loop, window and GL context are created.
The fonts are joined in `GlState::new` just before the cell renderer needs
them; the core in `resumed`, before the first paint. Rio's launcher core runs
in the PTY child and is unchanged.

After NUKE-111 the fonts cost only ~1.5 ms on a warm cache, so the scope was
widened to include `LauncherCore::new` (~4 ms inline). Both now finish
inside the ~30 ms of `GlState::new`; on the worker `LauncherCore::new` takes
9–12 ms (it competes with the driver's GL setup) but is off the critical
path.

### Method: interleaved A/B

Sequential runs of two builds drifted by several ms between sessions (a first
sequential balanced run showed Rio 1–4 ms *slower*, which an interleaved run
did not reproduce). The figures below therefore alternate a `main` build
(`ed14bcb`) and this branch open by open: 30 cold + 30 warm opens of each per
frontend and profile, in the same session.

### First-content present, p95 (ms)

| Open | Frontend | Perf main | Perf branch | Δ | Bal main | Bal branch | Δ |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| cold | native | 43.0 | 39.3 | −3.7 | 47.3 | 42.1 | −5.2 |
| warm | native | 51.6 | 39.0 | −12.6 | 52.4 | 46.2 | −6.2 |
| cold | rio | 74.9 | 73.4 | −1.6 | 78.1 | 77.1 | −1.0 |
| warm | rio | 80.5 | 82.9 | +2.4 | 86.4 | 84.3 | −2.1 |

Native is faster in every cell; native medians drop 5–9 ms. Rio, which gains
only the ~1.5 ms font load, is within noise (warm performance medians 75.3 vs
75.1 ms).

With an empty font cache (no `[font] path`, first open after fonts change),
the 73–78 ms fontconfig lookup now overlaps GL setup: native content arrives
at 83–85 ms in performance (was 113–116 ms) and 89–90 ms in balanced.

### Balanced vs performance (target ≤ 10%)

| Open | Frontend | Sequential run | A/B run, branch | A/B run, main |
| --- | --- | ---: | ---: | ---: |
| cold | native | +14.8% | +7.3% | +10.1% |
| warm | native | +8.4% | +18.4% | +1.5% |
| cold | rio | +6.4% | +5.1% | +4.3% |
| warm | rio | +5.9% | +1.7% | +7.3% |

Rio passes throughout. For native, the ratio swings by ~8 points between
sessions of the same build: the profile gap is 3–7 ms on a ~40 ms open, so a
few ms of p95 tail decide the verdict. Each native session misses the target
in one cell. Finding 4 (NUKE-115) is not settled by this: absolute balanced
native opens are faster than ever (42–47 ms p95), but a relative ≤ 10% target
at this size needs more samples, or an absolute threshold, to be meaningful.

What remains:

- `GlState::new` (28–34 ms) is the native open's critical path; the work
  overlapped here finishes inside it.
- Rio content still arrives ~30 ms after its blank frame (NUKE-113).

## After NUKE-113 — 2026-10-17

Branch `perf/rio-first-draw` (on `dca12a7`, which includes NUKE-112). Three
changes:

- The TUI loop (`tui_loop` in `src/main.rs`) draws its first frame before
  the first `poll_batch`, instead of after a 16 ms input wait.
- `GlState::new` is split. `GlState::open_window` creates the window and
  picks the GL config (~21 ms in performance, ~25 ms in balanced). `resumed`
  then joins the fonts, computes the grid and spawns the Rio child.
  `GlState::new` creates the context, surface and cell renderer afterwards
  (~8 ms). The child's exec, `App::new` and first draw now overlap that last
  part, instead of following it.
- The blank first present is kept: it is drawn at alpha 0, and the fade-in
  (`fade_step`) starts on the first frame with glyphs. In practice the child
  now finishes drawing (~32 ms) before GL setup does (~34 ms), so Rio's first
  present already carries content: `first-present` and
  `first-content-present` coincide.

Also fixed: the child dropped terminal resize events, so after a window
resize the Rio grid re-laid out only on the next key. `poll_batch` now
reports `Event::Resize` as an input, which triggers a redraw. This was
pre-existing on `main` and is unrelated to open latency.

### Where a Rio open goes now

Wall-clock trace of 10 cold Rio opens in performance, before and after
(medians in ms from process start; temporary instrumentation, not committed):

| Step | Before (`dca12a7`) | After |
| --- | ---: | ---: |
| window created | 24.1 | 23.6 |
| child spawned | 33.8 | 25.1 |
| child `App::new` done | 39.6 | 31.6 |
| child first draw | 56.5 | 31.9 |
| GL ready | ~32 | 33.9 |
| first present | 36.2 | 36.6 |
| first content present | 70.8 | 36.6 |

### First-content present, p95 (ms)

Interleaved A/B against `main` (`dca12a7`), 30 cold + 30 warm opens of each
per frontend and profile. The performance run was taken before the resize fix,
which does not touch the open path; the balanced run uses the final build.

| Open | Frontend | Perf main | Perf branch | Δ | Bal main | Bal branch | Δ |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| cold | rio | 74.0 | 40.6 | −33.4 | 79.9 | 45.7 | −34.2 |
| warm | rio | 77.6 | 44.4 | −33.2 | 86.8 | 53.1 | −33.7 |
| cold | native | 37.7 | 37.3 | −0.4 | 42.5 | 42.4 | −0.1 |
| warm | native | 46.3 | 43.7 | −2.6 | 47.8 | 47.7 | −0.1 |

Rio vs native on the branch (target: within 10 ms): performance +3.3 ms cold,
+0.7 ms warm; balanced +3.3 ms cold, +5.4 ms warm. **Passes in all four
cells.** Native is unchanged, as expected. Rio medians drop from 69–74 ms to
36–42 ms.

Hotpath reports (3 per profile) agree: Rio content at 35.5–37.5 ms in
performance and 41.9–43.4 ms in balanced, with `GlState::open_window`
21–26 ms, `GlState::new` 8–9 ms and `Session::spawn` 1.3–1.9 ms.

### Balanced vs performance

Balanced/performance p95 ratios on the branch: Rio +12.6% cold, +19.6% warm;
native +13.7% cold, +9.2% warm. Rio used to pass this target comfortably
because its ~30 ms blank-panel wait did not scale with the CPU profile. Now
that Rio opens as fast as native, it shows the same 3–9 ms profile gap on a
~40 ms open, and the same noise. As with NUKE-112, this is a relative target
on a small number. Absolute balanced opens are the fastest yet (Rio 46–53 ms
p95, previously 78–87 ms). NUKE-115 now applies to both frontends.

What remains:

- `GlState::open_window` (~21 ms, mostly winit/Wayland window creation and
  EGL config selection) is now the critical path for both frontends.
- No follow-up issue is proposed for Rio: the done-when target is met.

## After NUKE-114 — 2026-10-17

Branch `perf/probe-noop-keys` (on `c777c0a`, which includes NUKE-113). The
input probe now uses one rule for both frontends:

- Only a frame with a visible change answers an input. Rio already had this
  through its damage gate. Native now compares each frame's cells with the
  previous frame's (`LastFrame` in `src/gpu/window.rs`, only while
  `HYPRBURST_PERF` is on), so a key that repaints the same picture no longer
  closes a sample.
- A pending input expires when the next key arrives more than 50 ms later
  (`INPUT_EXPIRY` in `src/perf.rs`). It is printed as
  `query-input-unanswered` or `selection-input-unanswered` with its age, and
  the new key starts its own sample. Keys within 50 ms still fold, so bursts
  are measured from their first key.

The finding proposed dropping a sample "after one refresh with no damage".
That can't work on Rio: a no-op key makes the child send no PTY bytes, so the
parent gets no wake and no frame to time out on. The next key is the first
event that follows, so the rule is checked there.

One correction to finding 5: `Up` on the first row is not a no-op. Selection
wraps to the last row, so it changes the frame. `Backspace` on an empty query
and `Down` with a single result are true no-ops.

### Live sessions, performance (ms)

Typed manually, same script each time (query edits, `Backspace` on an empty
query, arrow keys, ~0.5–1 s pauses):

| Build | Frontend | Metric | n | median | p95 | max |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| before (`c777c0a`) | rio | query | 13 | 0.70 | 759.96 | 759.96 |
| before | rio | selection | 18 | 0.77 | 0.94 | 0.94 |
| before | native | query | 12 | 0.27 | 0.79 | 0.79 |
| before | native | selection | 14 | 0.28 | 0.35 | 0.35 |
| after | rio | query | 46 | 0.73 | 1.00 | 1.33 |
| after | rio | selection | 42 | 0.71 | 0.79 | 0.86 |
| after | rio | query unanswered | 1 | 346.6 | — | — |
| after | native | query | 16 | 0.34 | 0.52 | 0.52 |
| after | native | selection | 32 | 0.34 | 0.47 | 0.48 |

Before, one Rio query sample read 760 ms: a no-op `Backspace` answered by the
next key's frame. After, no answered sample in either frontend exceeds
1.4 ms; the no-op shows up as an unanswered line instead.

A targeted round on the final build (`Backspace` ×3 on an empty query, `f`,
`Backspace`, ~1 s apart) gives the same result in both frontends: three
`query-input-unanswered` lines (0.4–1.5 s, one per no-op key) and two
answered samples (Rio 0.81–0.90 ms, native 0.32–0.36 ms).

**Done when met:** no Rio sample above ~20 ms comes from a no-op key, and
`docs/hotpath-profiling.md` describes the rule. Rio medians are unchanged;
native medians rise ~0.06 ms (0.27–0.28 → 0.34 ms), consistent with the
probe-only cell comparison, which normal runs don't pay.

Remaining limit: a no-op key followed by another key within 50 ms still
folds, so that sample can read up to ~50 ms. Real input latency here is
1–2 ms, so such a sample stands out.

## After NUKE-115 — 2026-10-17

Re-measure of finding 4 on `main` (`d703172`, which includes NUKE-111..114),
no code change. Earlier sessions of 30 opens swung the balanced/performance
ratio from 1.5% to 20%, so this run uses 100 cold + 100 warm `--measure`
opens per frontend and profile, one throwaway open first, performance and
then balanced in one session.

### First-content present (ms)

| Open | Frontend | Perf median | Perf p95 | Bal median | Bal p95 | p95 Δ | Ratio | ≤ 10% |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | :---: |
| cold | native | 34.9 | 38.0 | 39.2 | 43.4 | +5.4 | +14.1% | fail |
| warm | native | 36.5 | 46.4 | 39.0 | 50.6 | +4.2 | +9.0% | **pass** |
| cold | rio | 36.5 | 39.4 | 41.1 | 44.9 | +5.6 | +14.1% | fail |
| warm | rio | 37.9 | 45.1 | 41.0 | 50.1 | +5.0 | +11.0% | fail |

Warm native in balanced, the case finding 4 was about, is now +9.0% above
performance: **NUKE-115's done-when is met.** It was 103.9 ms p95 at the
baseline and is 50.6 ms now. Its long tail came from the font lookup and GL
setup that NUKE-111 and NUKE-112 removed or overlapped, as the finding
expected.

The other three cells miss the relative target by 1–4 points. In every cell
the gap is 4–6 ms on a ~40 ms open, consistent with a lower clock slowing CPU
and driver work this short. Five hotpath reports of a cold native open per
profile show the same phases in both (`GlState::open_window` 20–23 ms,
`GlState::new` 7.6–10.9 ms, content at 33.4–40.0 ms). At five samples the
phase noise is larger than the gap, so hotpath cannot place it in one phase.

A relative ≤ 10% target no longer separates a regression from clock speed
at this size: a 4 ms gap is 10% of a 40 ms open. For future runs, an
absolute bound (balanced p95 ≤ performance p95 + 10 ms) or a balanced p95
budget (≤ 60 ms) would catch a real regression without flagging the clock
ratio.

## Follow-ups

Filed as sub-issues of NUKE-109:

| Issue | Finding | Blocked by | Status |
| --- | --- | --- | --- |
| [NUKE-111](https://linear.app/tiendacables/issue/NUKE-111) Cache font resolution and skip the Nerd Font scan | 1 | — | fixed (see above) |
| [NUKE-112](https://linear.app/tiendacables/issue/NUKE-112) Load fonts while the window and GL context are created | 2 | NUKE-111 | fixed (see above) |
| [NUKE-113](https://linear.app/tiendacables/issue/NUKE-113) Rio: draw before the first input poll, spawn the child earlier | 3 | — | fixed (see above) |
| [NUKE-114](https://linear.app/tiendacables/issue/NUKE-114) Probe: count no-op keys the same way in both frontends | 5 | — | fixed (see above) |
| [NUKE-115](https://linear.app/tiendacables/issue/NUKE-115) Re-measure warm native open in balanced | 4 | NUKE-111, NUKE-112 | resolved: warm native +9.0% at n=100 (see above) |
