# Rio VT Prototype Findings

## Decision

Hyprburst uses the safe Rust `rio-vt` crate rather than `librio`. Both expose
Rio's terminal engine, but `librio` adds a C ABI intended for non-Rust hosts.
Using `rio-vt` keeps the integration type-safe and avoids an unnecessary FFI
boundary, static library, generated header, and `unsafe` ownership contract.

After manual validation, the Rio-backed implementation became the default. Run
the retained direct in-process fallback with:

```sh
hyprburst native
```

## Architecture

The prototype keeps responsibilities deliberately narrow:

1. Hyprburst opens its existing winit, glutin, and OpenGL window with the normal
   configured app ID, dimensions, transparency, and Hyprland placement handoff.
2. `rio-vt` creates a PTY and starts the current Hyprburst executable with the
   `tui` command.
3. Rio's `Machine` reader thread parses the child's terminal output into a
   `Crosswords` grid and wakes the winit event loop when the grid is damaged.
4. Hyprburst snapshots visible Rio cells and adapts glyphs, ANSI colors,
   backgrounds, and bold state to the existing OpenGL cell renderer.
5. Text, launcher navigation keys, PTY resize messages, and terminal-generated
   replies are sent back through Rio's PTY channel.
6. Closing the window shuts down the PTY and child; child exit closes the window.

The `rio-vt` default feature set is intentionally empty, so this prototype does
not pull in Rio's renderer, GPU, font shaping, clipboard, or window stack.

## Measurement

Measured on 2026-07-30 from the same optimized binary in a live Wayland and
Hyprland session:

```text
variant  cold start to first presented frame
gui      66.494 ms
rio-vt   66.891 ms
```

Commands:

```sh
target/release/hyprburst --measure
target/release/hyprburst native --measure
```

The Rio path was 0.397 ms slower in this single comparison. Both results
are below the existing 250 ms CI startup ceiling, although that ceiling guards
config loading and launcher initialization rather than a live Wayland frame.
The native path remains above the aspirational 50 ms local target on this
particular run, so these figures should be treated as comparative rather than a
portable benchmark.

## Limitations

- The prototype adds a PTY and a second Hyprburst process instead of running the
  launcher state machine directly in the window process.
- It forwards the text and navigation keys needed by the launcher, but does not
  yet implement a general terminal key encoder for modifiers or function keys.
- Clipboard, selection, scrollback UI, mouse reporting, search, and Rio image
  protocols are intentionally out of scope.
- The renderer repaints the visible grid when Rio reports damage; it does not yet
  consume Rio's per-row dirty snapshot API.
- Peak RSS from the existing probe is process-scoped and does not provide a
  reliable aggregate for the parent plus PTY child.

## Promotion Decision

Promote the Rio-backed path to bare `hyprburst` after successful manual testing
of typing, navigation, launch, resize, and close behavior. Keep `hyprburst
native` as a fallback and performance comparison. The promotion favors Rio's
working terminal semantics and reusable engine with near-parity in the latest
cold-start comparison, despite the extra child process.

Future work should add complete key encoding, aggregate memory measurement,
dirty-row rendering, and repeated cold/warm benchmarks. Those limitations do not
block the launcher interactions exercised by the default path.
