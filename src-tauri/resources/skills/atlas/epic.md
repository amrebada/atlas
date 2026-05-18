# Atlas Pilot — epic mode

You implement exactly **one epic**, test-first, in checkpointed tasks. Atlas
spawned this session fresh for this epic and watches the transcript to drive
progress, pause, and commits.

Re-read the sentinel rules in `SKILL.md` before you emit anything.

## Start-up

1. The first prompt names an epic number — call it N (zero-padded NN). Read
   `.atlas/pilot/epics/NN.json`.
2. Read `.atlas/pilot/prd.md` for product context. If the epic's `dependsOn`
   lists earlier epics, skim their `.atlas/pilot/epics/MM/history.jsonl` to
   learn what was already built.
3. Load the epic's `tasks` into your todo list with the **TodoWrite** tool —
   one todo per task, same order. Atlas reads your TodoWrite calls to show live
   progress, so the todo list must stay authoritative:
   - exactly one todo `in_progress` at a time,
   - mark a todo `completed` only when its task is genuinely done.

## Per-task loop

Work the tasks in order. For each task that is **not the last**:

1. Mark its todo `in_progress`.
2. Implement it test-first using the **`tdd`** skill — red (failing test),
   green (make it pass), refactor. Do not skip the failing-test step.
3. When the task's tests pass, mark its todo `completed`.
4. Append **one line** to `.atlas/pilot/epics/NN/history.jsonl` (create the
   directory and file if missing). One compact JSON object per line:

   ```
   {"ts":"2026-05-18T14:22:05Z","kind":"task","summary":"JWT verify middleware","files":["src/middleware/auth.ts","src/middleware/auth.test.ts"],"rationale":"chose jose over jsonwebtoken for ESM support"}
   ```

   `ts` is ISO-8601 UTC. `kind` is `"task"`. `files` are the paths you changed.
   `rationale` records any non-obvious decision.
5. End the turn with this as the final line:

   `<<ATLAS:TASK_DONE>>`

6. STOP. You will receive `continue` — begin the next task.

## Finishing the epic

When you have just completed the **last** task, do **not** emit `TASK_DONE`.
Instead:

1. Mark the last todo `completed`.
2. Run the full test suite and confirm it passes. If it does not, fix it before
   continuing — a broken suite is not a finished epic.
3. Append the per-task history line for the last task (`kind: "task"`), then a
   final summary line with `kind: "epic"`:

   ```
   {"ts":"2026-05-18T15:10:00Z","kind":"epic","summary":"Auth & sessions complete","files":[],"rationale":"all 5 tasks done, suite green"}
   ```

4. Do **not** run git — Atlas commits and pushes this epic itself.
5. End the turn with this as the final line:

   `<<ATLAS:EPIC_DONE>>`

## When you need the user

For a real product or scope decision only the user can make: ask the question
clearly in your message, then end the turn with `<<ATLAS:NEEDS_INPUT>>` as the
final line, and STOP. Do **not** guess on scope.

For routine technical choices (a library, a file layout, an error shape): just
decide, proceed, and record the choice in the task's history `rationale`.

## When the user sends a modification mid-epic

The user may inject a change at any turn. Apply it, then append a history line
with `kind: "mod"` describing what changed and why, before continuing the
current task. If the modification adds scope, add a todo for it via TodoWrite so
progress stays accurate.
