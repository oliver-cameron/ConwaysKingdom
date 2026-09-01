# Simplifying

How to cut this tree back, and what not to cut. Written down because the same
judgement has to be made a few hundred times and it should be made the same way
each time.

## The test for one comment

Ask what happens if it is deleted.

| the answer | what to do |
|---|---|
| Nothing — the code says it | delete it |
| A reader has to go and look at `docs/` | replace it with a link |
| Somebody re-introduces a fault | **keep it**, in as few words as it takes |
| A reader learns how the code used to be | delete it, unless it is the line above |

The third row is the whole of what a comment is for here. `// Sorted, so two
peers agree` earns its place because an unsorted `HashMap` is a desync nobody
would find; `// Sorted` above `.sort()` does not.

## Where the cut material goes

Not always the bin.

- **An argument about a design** belongs in `docs/` — [simulation.md](simulation.md),
  [game.md](game.md), [networking.md](networking.md). The code links to it.
- **A symptom that turned out to mean something else** belongs in
  [gotchas.md](gotchas.md). That file is the record of what a day cost, and it
  is the one to read when something looks familiar.
- **An intention** belongs in [planned.md](planned.md), with a status on it.

Moving is the default for anything that took real work to find out. Deleting is
for the retelling.

## When the comment is not the problem

A block you cannot shorten is usually a structure you cannot see.

The hotbar drew its squares with a counter incremented by hand between nested
closures, and `shifted` was a *second* list that had to come out in the same
order. Every attempt to explain that was a paragraph. Writing the bar down as a
list — `slots`, one entry per square, read by the keyboard and the layout alike
— left nothing to explain, and took the file from 36% comment to 26% without
losing a sentence worth keeping.

So: if the comment is hard to write, try moving the code first.

## Shapes worth reaching for

- **A list rather than a sequence of calls.** Data can be read, tested, and
  walked by more than one caller; a sequence of calls can only be read.
- **One source, derived twice.** Two lists that must agree will stop agreeing.
- **Pure, then drawn.** `slots`, `covered`, `fit`, `plan` — a function with no
  egui and no GPU in it is one a test can reach.
- **Name the number.** A constant with a name is a comment that cannot go stale.

## Finding the bloat

Comment blocks of eight lines or more, longest first:

```sh
python3 - <<'PY'
import pathlib
for p in sorted(pathlib.Path('src').rglob('*.rs')):
    lines = p.read_text().splitlines()
    n = start = 0
    for i, l in enumerate(lines):
        if l.strip().startswith(('///', '//!', '//')):
            if n == 0:
                start = i
            n += 1
        else:
            if n >= 8:
                print(f"{p}:{start + 1}  {n} lines")
            n = 0
PY
```

And the ratio per file, to see where to start:

```sh
for f in $(find src -name '*.rs'); do
  printf '%3d%%  %s\n' $(( $(grep -cE '^\s*(///|//!|//)' "$f") * 100 / $(wc -l < "$f") )) "$f"
done | sort -rn | head -20
```

## What good looks like

A doc comment is a summary line, then the reason it is not the obvious thing,
then nothing. Two paragraphs is a lot. Four is a `docs/` page with the code
linking to it.

The rule is not a budget — `sim/rule.rs` is a third comment and every line of it
is a number's meaning, which is exactly right. It is that a reader should be
able to see the code.
