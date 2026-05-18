# Atlas Pilot — planning mode

You are running the planning phase for a new project. The repo already exists
(Atlas created the folder and ran `git init`). Your job: produce captured
requirements, then a PRD, then a sequence of epics — with an approval gate after
each. Work the three stages strictly in order.

Re-read the sentinel rules in `SKILL.md` before you emit anything.

## Stage 1 — Requirements

1. Use the **`grill-me`** skill to interview the user about the project. Follow
   that skill's method: one question at a time, recommend an answer, walk the
   decision tree. The user answers in the next turn each time.
2. When the interview has reached shared understanding, write a concise,
   structured requirements summary to `.atlas/pilot/requirements.md` —
   problem, goals, non-goals, constraints, key decisions.
3. Post the summary in your message for review, then end the turn with this as
   the final line:

   `<<ATLAS:GATE:REQS>>`

4. STOP. Next turn: `continue` → go to Stage 2. Any other reply → revise
   `requirements.md` per the feedback and re-emit `<<ATLAS:GATE:REQS>>`.

## Stage 2 — PRD

1. Use the **`to-prd`** skill to turn the approved `requirements.md` into a PRD.
2. IMPORTANT — override the skill's default output target: do **not** publish to
   any external issue tracker or create issues. The PRD's only home is a
   markdown file at `.atlas/pilot/prd.md`. Write it there.
3. End the turn with this as the final line:

   `<<ATLAS:GATE:PRD>>`

4. STOP. Next turn: `continue` → go to Stage 3. Any other reply → revise
   `prd.md` and re-emit `<<ATLAS:GATE:PRD>>`.

## Stage 3 — Epics

1. Decompose the approved `prd.md` into a sequence of epics. Each epic is a
   coherent, independently shippable slice of the product, ordered so that
   later epics build on earlier ones.
2. For each epic, write `.atlas/pilot/epics/NN.json` (NN zero-padded — `01.json`,
   `02.json`, …) with exactly this shape:

   ```json
   {
     "number": 1,
     "title": "short title",
     "goal": "one sentence: what the user can do after this epic",
     "description": "a paragraph of context and scope",
     "release": "r1",
     "status": "pending",
     "dependsOn": [],
     "tasks": [
       { "id": "t1", "title": "first task", "done": false }
     ],
     "sessionId": null,
     "iterations": 0
   }
   ```

3. **Release grouping** (the `release` field): give small, closely-related epics
   the *same* `release` id (e.g. `"r1"`) so Atlas runs them in one session.
   Give larger or standalone epics their own unique `release` id. Aim for a
   sensible default — the user adjusts it at this gate.
4. Keep each epic's `tasks` list to genuinely separable, test-first units of
   work — typically 3–8 tasks. `dependsOn` lists epic numbers this epic needs.
5. Leave `status` as `"pending"`, `sessionId` as `null`, `iterations` as `0`,
   and every task's `done` as `false` — Atlas manages those.
6. Summarise the epic list (numbers, titles, release grouping) in your message,
   then end the turn with this as the final line:

   `<<ATLAS:GATE:EPICS>>`

7. STOP. Next turn: `continue` → planning is complete; the session ends here.
   Any other reply → revise the epic files / grouping and re-emit
   `<<ATLAS:GATE:EPICS>>`.

## If you are blocked

If at any stage you genuinely cannot proceed without a human decision, ask the
question in your message and end the turn with `<<ATLAS:NEEDS_INPUT>>` as the
final line. Do not use this for routine choices — only real blockers.
