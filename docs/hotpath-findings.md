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

`GlState::new` (EGL display, config, context, surface) takes ~29 ms, and runs
after `resolve_fonts`. Neither needs the other until the cell renderer is
built.

Proposed fix: resolve and load fonts on a thread while the window and GL
context are created. Expected impact: up to the shorter of the two (~29 ms
today, less once finding 1 lands) off first presentation.

### 3. Rio shows an empty panel for ~30 ms; the child waits a frame before drawing

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

Back-to-back native opens in balanced have a long tail (p95 103.9 ms, max
110 ms vs 91–92 ms in performance). The cold case and both Rio cases pass. The
open path is dominated by findings 1 and 2, which are process spawns and driver
work sensitive to clock ramp-up; removing them should shrink both the absolute
cost and the profile gap. Re-measure after 1 and 2 before treating this
separately.

### 5. The probe folds no-op keys into the next change on Rio

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

## Follow-ups

Drafted for approval, not yet filed:

1. Cache font resolution and skip the Nerd Font scan (finding 1).
2. Overlap font loading with GL context creation (finding 2).
3. Rio: first draw before first poll, earlier child spawn (finding 3).
4. Probe: consistent no-op key handling across frontends (finding 5).
