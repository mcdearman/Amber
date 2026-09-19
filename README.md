# insta

Snapshot testing for [Meadow](https://github.com/mcdearman/meadow). A test
asserts that a value matches the snapshot stored for it. When the value
changes, the test fails and the new value is stored beside the old one for you
to review.

This package is a port of Rust's [`insta`](https://github.com/mitsuhiko/insta)
1.48.0:

- it reads and writes the same `.snap` files, YAML header included;
- it obeys the same `INSTA_*` environment variables and `insta.yaml`;
- it prints the same summaries and diffs.

## Install

```sh
meadow add mcdearman/MeadowInsta
```

## Use

```meadow
use Insta (assertSnapshot, assertInlineSnapshot)

fun report n = "total: ${n}\nitems:\n  - apples\n  - pears\n"

@test fun rendersTheReport () = assertSnapshot "report" (report 2)

@test fun inlineValue () = assertInlineSnapshot "a\nb" "\n    a\n    b\n    "
```

The first run fails and stores `snapshots/report.snap.new`:

```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━ Snapshot Summary ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Snapshot file: snapshots/report.snap
Snapshot: report
────────────────────────────────────────────────────────────────────────────────
Expression: report
────────────────────────────────────────────────────────────────────────────────
+new results
────────────┬───────────────────────────────────────────────────────────────────
          1 │+total: 2
          2 │+items:
          3 │+  - apples
          4 │+  - pears
────────────┴───────────────────────────────────────────────────────────────────
stored new snapshot snapshots/report.snap.new
```

To accept a new snapshot, do one of:

- rename the file to `report.snap`;
- call `acceptPending "snapshots"`, which does that for every pending file;
- run the tests again with `INSTA_UPDATE=always`.

After that, the test passes until the value changes, and then it prints a diff.

### What controls it

The environment, and `insta.yaml` in the workspace (`behavior:` keys), work as
they do for the crate:

| variable | |
|---|---|
| `INSTA_UPDATE` | `auto` (the default), `always`, `new`, `unseen`, `no` or `force`: whether a changed snapshot is written in place, stored as `.snap.new`, or left alone. `auto` stores `.snap.new`, except under CI (`CI`, `TF_BUILD`), where it writes nothing |
| `INSTA_OUTPUT` | `diff`, `summary`, `minimal` or `none` |
| `INSTA_FORCE_PASS` | `1` to report failures without failing |
| `INSTA_REQUIRE_FULL_MATCH` | `1` to also compare the metadata |
| `INSTA_PENDING_DIR` | a directory to store `.snap.new` files under instead |
| `INSTA_WORKSPACE_ROOT` | where paths are relative to (default `.`) |
| `INSTA_YAML_BLOCK_STYLE` | `1` to write multi-line metadata as literal blocks |
| `CLICOLOR_FORCE` | colours in the output |

Rust gives insta a test's name, file and line; Meadow does not. So:

- a snapshot is named by its first argument;
- `assertSnapshotWith settings name expression value` places and describes it
  from `settings ()` updated as you like:
  - `sourceFile`: the test's file, whose directory holds `snapshotPath`, which
    is `snapshots` by default;
  - `modulePath`, prepended to file names;
  - `snapshotSuffix`, `description` and `info` (a `Yaml` value);
  - `omitExpression`, `line`, `colors` and `width`.

`assertInlineSnapshot value reference` compares against a literal in the test.
When the value differs, it prints the new value as a Meadow string literal to
paste in.

### The parts

The pieces the assertions are built from are exported too:

- `parseSnapshot` and `serialize` for `.snap` files;
- `contents`, `normalize`, `matches`, `matchesFully`, `fromInlineLiteral` and
  `toInline` for comparing snapshots;
- `printer` and `printedText` for insta's output;
- `toYaml` and `parseYaml` for the YAML metadata.

## How it's made

The files in `src/` are hand translations of the crate:

- `src/Yaml.mw`: the YAML emitter insta vendors from yaml-rust, and a reader
  for the block YAML that snapshot headers use;
- `src/Snapshot.mw`, `src/Output.mw` and `src/Runtime.mw`: insta itself.

**`src/Cases.mw`** is generated test data:

- 500 values written as YAML;
- 430 snapshot files read back, with headers of many shapes and the legacy
  format;
- 600 pairs of snapshot contents, compared and written as inline literals;
- 400 whole assertions, each run by insta in a child process, with what it
  printed and the files it left.

Run `scripts/generate.sh` to regenerate; it needs a Rust toolchain.

## Licence

Apache-2.0, like insta: see [LICENSE](LICENSE) and [COPYRIGHT](COPYRIGHT).
