---
name: atlas
description: >-
  Drives an Atlas Pilot project through its lifecycle. Use this skill whenever a
  prompt says "Use the atlas skill" — in planning mode (interview → PRD → epics)
  or epic mode (implement one epic, test-first). The skill reads and writes
  .atlas/pilot/ and emits ATLAS sentinel lines the Atlas desktop app watches for.
---

# Atlas Pilot

This skill is invoked by the **Atlas** desktop app to drive a software project's
lifecycle. Atlas wraps this Claude Code session and observes it **only by reading
the session transcript** — it cannot see your screen or your tool calls' side
effects directly. Anything Atlas needs to know, you must put into the transcript
using the conventions below. Follow them exactly; the automation depends on it.

## Modes

The first prompt of the session names a mode. Pick one and follow its file:

- **"planning mode"** → read `plan.md` (in this skill directory) and follow it.
- **"epic mode"** → read `epic.md` (in this skill directory) and follow it.

Read the matching mode file now, before doing anything else.

## The sentinel protocol

A *sentinel* is a control line Atlas greps for in the transcript. Rules — all of
them matter:

1. A sentinel MUST be the **entire final line** of your message. Nothing may
   follow it — no punctuation, no whitespace, no closing remark.
2. Emit **exactly one** sentinel per turn, and only when a mode file tells you to.
3. Never put a sentinel inside backticks, quotes, or a code block. It must be
   raw text on its own line.
4. After emitting a `GATE:*` or `NEEDS_INPUT` sentinel, **stop and wait**. The
   next turn brings a reply — often just the word `continue`.

| Sentinel | Meaning |
|----------|---------|
| `<<ATLAS:GATE:REQS>>`  | Planning: requirements captured, awaiting approval |
| `<<ATLAS:GATE:PRD>>`   | Planning: PRD written, awaiting approval |
| `<<ATLAS:GATE:EPICS>>` | Planning: epics generated, awaiting approval |
| `<<ATLAS:TASK_DONE>>`  | Epic: one task finished, ready for the next |
| `<<ATLAS:EPIC_DONE>>`  | Epic: every task finished, epic complete |
| `<<ATLAS:NEEDS_INPUT>>`| Either mode: blocked, a human must answer |

## The `.atlas/pilot/` layout

All pilot state lives under `.atlas/pilot/` in the repo. Ownership matters —
only write the files marked "you write":

```
.atlas/pilot/
  project.json            Atlas owns this. Do not write it.
  requirements.md         you write  (planning, gate REQS)
  prd.md                  you write  (planning, gate PRD)
  epics/NN.json           you write  (planning, gate EPICS; NN = 01, 02, …)
  epics/NN/history.jsonl  you append (epic mode)
```

Do not commit anything. Atlas runs git itself at epic boundaries.

## Replies you will receive

- `continue` — the gate/task was approved; proceed to the next step.
- Anything else — feedback or a modification. Apply it, then re-emit the
  sentinel for the current step (planning) or fold it in and continue (epic).
