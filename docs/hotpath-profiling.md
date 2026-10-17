# Profiling with hotpath

[hotpath](https://hotpath.rs/) is an opt-in profiler: built with its cargo
features, hyprburst reports per-function timing, allocations, and per-thread
CPU and memory when the launcher exits. Without the features every
`#[hotpath::measure]` and profiling guard compiles to the plain code and none
of hotpath's dependencies are built.

It complements `HYPRBURST_PERF`, the end-to-end latency probe below:
`HYPRBURST_PERF` measures what the user waits for (process start to first
presented frame, key press to the frame that shows it), hotpath explains where
the time and memory inside those spans go. `--measure` stays the one-shot
cold-open and footprint check.

Measured findings live in [Hotpath findings](hotpath-findings.md).

## Features

| Feature | Report |
| --- | --- |
| `hotpath` | function timing and threads |
| `hotpath-alloc` | all of the above plus allocated bytes per function (`HOTPATH_ALLOC_METRIC=count` for allocation counts) |

The report is written when the launcher closes normally (`Escape`, or a
launch); a killed process writes none. It goes to stdout unless redirected with
`HOTPATH_OUTPUT_FORMAT=json` and `HOTPATH_OUTPUT_PATH=<file>`.

## Running

### Offscreen, repeatable, no compositor needed

`examples/query-cost.rs` drives a launcher core through typing, deleting, and
moving the selection, rendering each step into a buffer, and prints median /
p95 / max per action kind:

```sh
cargo run --release --example query-cost -- [--live] [rounds] [idle-ms]
cargo run --release --example query-cost --features hotpath-alloc -- --live 40 200
```

The default app set is a fixed synthetic one (300 entries) so numbers compare
across machines; `--live` uses this machine's discovered apps. `idle-ms` sleeps
between steps, so each one is the cold burst after idle a person typing causes
instead of a tight loop that keeps the core clocked up.

### Live window

Normal launches hand off to Hyprland (`hyprctl dispatch exec_cmd`) so window
rules apply; that child is spawned by the compositor and inherits neither this
shell's environment nor its stderr. Set `HYPRBURST_CHILD=1` to open the window
directly instead. It then gets the compositor's default placement, not the
configured floating rules, so the grid size may differ from daily use.

```sh
cargo build --release --features hotpath-alloc
HYPRBURST_CHILD=1 HYPRBURST_PERF=1 \
HOTPATH_OUTPUT_FORMAT=json HOTPATH_OUTPUT_PATH=/tmp/hb-hotpath.json \
  target/release/hyprburst 2>>/tmp/hb-perf.log          # Rio default
HYPRBURST_CHILD=1 HYPRBURST_PERF=1 \
HOTPATH_OUTPUT_FORMAT=json HOTPATH_OUTPUT_PATH=/tmp/hb-native.json \
  target/release/hyprburst native 2>>/tmp/hb-perf.log   # direct fallback
scripts/perf-summary.sh /tmp/hb-perf.log
```

Type a query, delete it, and move the selection; close with `Escape`, which
writes the report. Repeat until each metric has at least 30 samples.

The default frontend is two processes: the GPU window, and a `hyprburst tui
--gpu-host` child on a PTY that owns the launcher state. Each writes its own
report. The child's goes next to the parent's with a `-tui-<pid>` suffix
(`/tmp/hb-hotpath-tui-<pid>.json` above), or to
`$TMPDIR/hyprburst-hotpath-tui-<pid>.txt` when `HOTPATH_OUTPUT_PATH` is unset,
because its stdout is the PTY the parent paints. The child's metrics server is
turned off for the same reason.

Only launcher runs are profiled: the guards sit on the Rio, native, and TUI
entry points in `main.rs`, after the Hyprland hand-off, so a dispatching parent,
`--bench-startup`, and `help` print no report.

## `HYPRBURST_PERF`

With `HYPRBURST_PERF=1` (any value but empty or `0`) the GPU window prints one
stderr line per sample, `hyprburst-perf <metric> <frontend> <ms>`, where
`<frontend>` is `rio` or `native`:

| Metric | From | To |
| --- | --- | --- |
| `first-present` | process start (`main`) | the first presented frame (what `--measure` reports) |
| `first-content-present` | process start (`main`) | the first presented frame that draws any glyph |
| `query-input-to-present` | a typed character, `Backspace`, or `Tab` | the first presented frame with a visible change |
| `selection-input-to-present` | an arrow key or `PageUp`/`PageDown` | the first presented frame with a visible change |

The Rio window presents its first frame before the PTY child has drawn
anything, so for `rio` only `first-content-present` is when the launcher becomes
visible; for `native` the two coincide. With the probe on, `--measure` stays
open until content is presented.

Keys pressed before that frame are folded into the first one, so a burst is
measured from its first key. For the Rio frontend the change has to come back
from the child over the PTY, so this includes the child's filtering and
rendering, the PTY round trip, and the parent's VT parse and draw. A key that
changes nothing on screen (e.g. `Down` with one result) is answered by the next
visible change, so take the p95, not the max, as the figure of record. With the
probe unset, nothing is recorded.

`scripts/perf-summary.sh` turns one or more logs into n / median / p95 / max
per metric and frontend; other stderr lines are ignored.

### Cold and warm open

Each open is a fresh process. Without root the page cache cannot be dropped,
so both cases run with binaries, fonts, and `.desktop` files cached:

- **cold**: the first open after at least 2 s idle, so the CPU has clocked down
  and its caches are cold — what pressing the launcher key costs;
- **warm**: an open started immediately after the previous one exits.

```sh
for i in $(seq 30); do sleep 2; HYPRBURST_CHILD=1 HYPRBURST_PERF=1 \
  target/release/hyprburst --measure >/dev/null 2>>/tmp/hb-cold.log; done
for i in $(seq 30); do HYPRBURST_CHILD=1 HYPRBURST_PERF=1 \
  target/release/hyprburst --measure >/dev/null 2>>/tmp/hb-warm.log; done
```

## What is instrumented

- Open and first presentation: `Config::load`, `discover_apps`,
  `History::open`, `load_scores`, `LauncherCore::new`, `resolve_fonts`,
  `GlState::new`, `CellRenderer::new`, and `Session::spawn` (the Rio PTY child).
- Query and filter: `LauncherCore::{apply,rebuild_filtered}` and
  `filter_and_rank`.
- Selection and render: `LauncherCore::{move_vertical,move_horizontal,view}`,
  `render_core`, `build_cells` (native), `Session::frame_into` (Rio damage
  snapshot), `CellRenderer::{draw,ensure_glyph}`, and in the TUI child
  `App::{handle_key,render}`.

`tests/hotpath_profile.rs` (run with `--features hotpath`, and in CI) fails if
the open, query, or render instrumentation is lost.
