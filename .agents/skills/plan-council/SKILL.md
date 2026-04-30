---
name: plan-council
description: Use this skill immediately after drafting or substantially revising a plan file. It spawns a council of four specialized subagents (contrarian, gap-finder, detail analyst, question-raiser) to stress-test the plan in parallel, surfaces their findings and open questions to the user for input, then synthesizes a consensus view and updates the plan file in place. Do NOT use before a plan exists, for trivial single-step tasks, or mid-execution.
---

# Plan Council

Run a four-lens review on a freshly drafted plan before execution begins. The goal is to catch blind spots, force explicit decisions on ambiguities, and produce a plan the user has actually pressure-tested — not one that only feels complete.

## When to use

- A plan file has just been created or meaningfully updated.
- The plan is non-trivial: multiple steps, architectural choices, or irreversible decisions.
- Execution has not yet started.

## When NOT to use

- No plan file exists yet — draft one first.
- Single-step or trivial tasks.
- Mid-execution course-correction — this is a pre-flight review, not a debugger.
- The user has already explicitly approved the plan and asked to proceed.

## Workflow

### Step 1 — Prepare the shared brief

Assemble a brief that every subagent will receive. It must include:

- The full plan file contents, verbatim.
- The original user goal and problem statement.
- Hard constraints: budget, timeline, stack, non-goals, known limitations.
- The absolute path of the plan file.

Do not summarize the plan for the subagents. They must see the same text the user will ultimately execute against.

### Step 2 — Spawn the four subagents in parallel

Launch all four in a single batch. Do **not** run them sequentially — their outputs must be independent to avoid anchoring on each other.

**Agent 1 — The Contrarian**

> Steelman the opposition. Assume this plan fails. Identify every load-bearing assumption and challenge it. Where is the plan most likely to break? What failure mode is the author underweighting? What would a skeptical senior engineer say in code review? If there is a fundamentally different approach worth considering, name it.

**Agent 2 — The Gap-Finder**

> Find what is missing. Look at the plan from the outside. What features, requirements, edge cases, or user needs are absent? What adjacent capabilities would make this 2–10x more valuable? What non-functional requirements — security, observability, error handling, accessibility, performance, migration, rollback — are unaddressed? What happens on day 2, not day 1?

**Agent 3 — The Detail Analyst**

> Read the plan line by line for technical correctness. Are dependencies ordered correctly? Are the technical claims accurate? Are any steps contradictory, duplicated, or vague? Are estimates realistic? Are there specific implementation risks the author has not called out? Cite line numbers or section headings wherever possible.

**Agent 4 — The Question-Raiser**

> Identify the 5–10 ambiguities that most change the shape of this plan if resolved. Focus on assumptions about the user, the system, the constraints, or the success criteria. Each question must be answerable in one or two sentences. Rank by how much the answer would change the plan.

### Step 3 — Required output format (identical for every subagent)

Each subagent must return exactly this structure:

```
FINDINGS
- [concise point]
- [concise point]

SEVERITY
- [Critical | High | Medium | Low] for each finding above, in order

SUGGESTED CHANGES
- [specific, actionable edit to the plan, quoting the section being changed]

OPEN QUESTIONS
- [question for the user, or "none"]
```

Reject and re-run any subagent that returns prose instead of this structure.

### Step 4 — Deduplicate and cluster

Collect all four outputs. Then:

1. Merge near-duplicate findings across agents.
2. Group by theme: architecture, scope, risk, UX, ops, data, security, etc.
3. Tag any finding raised by two or more agents as **consensus** — weight it higher in Step 6.
4. Preserve attribution — know which agent raised what, in case the user wants to drill in.

### Step 5 — Surface to the user and stop

Post a single consolidated message containing:

1. A 3–5 line summary of what the council found.
2. Top findings grouped by theme, leading with consensus items. Keep each finding to one line.
3. Open questions as a numbered list, ranked by leverage (most plan-shaping first).
4. An explicit ask:
   > "Please answer any of the questions above — or tell me which to skip. I will update the plan based on your answers and the council's consensus."

**Stop here.** Do not proceed to synthesis until the user has responded or explicitly said to proceed without answers. Do not silently pick answers for the user.

### Step 6 — Synthesize consensus

Once the user has responded:

- User answers are authoritative. They override any subagent opinion.
- For findings the user did not address, default to **consensus** (two or more agents agreeing) as the tiebreaker.
- For single-agent findings, adopt only if Critical/High severity or cheap to incorporate.
- Write a 5–10 bullet synthesis capturing the agreed direction.
- If the contrarian surfaced a fundamentally different approach and the user dismissed it, record that dismissal with its rationale — do not silently drop it.

### Step 7 — Update the plan file

Edit the plan file in place. Do not rewrite it from scratch.

- Preserve the original structure and the author's intent.
- Modify steps where the council and user input agreed on a change.
- Where a step is changed, keep a brief note of what it was and why it changed.
- Append a `## Council Review` section at the bottom containing:
  - Review date.
  - Key changes made, each with a one-line rationale.
  - Deferred findings — things raised but explicitly not adopted, with reasoning.
  - Remaining open questions, if any.

### Step 8 — Confirm back to the user

Post a short closing message:

- What changed in the plan (bullet list).
- What was deferred and why.
- Any remaining open questions.
- The absolute path of the updated plan file.

## Anti-patterns to avoid

- **Running the council sequentially.** Agents will anchor on each other and you lose independent signal.
- **Pre-filtering findings before showing the user.** Surface everything in Step 5; let the user prioritize.
- **Silently answering your own open questions.** If a subagent flagged it as important, the user decides.
- **Rewriting the plan from scratch.** Edit in place. The original author's intent matters.
- **Skipping Step 5.** The user's input is the highest-signal input in the entire workflow.
- **Treating contrarian output as noise.** The contrarian exists specifically to find what you missed. Take it seriously even when it feels wrong.
- **Collapsing the four roles into one "reviewer" agent.** The roles are deliberately overlapping-but-distinct; running one agent loses the diversity that makes this work.

## Tuning knobs

- **Small plans (under one page):** collapse severity categories and skip thematic grouping.
- **Very large plans:** run the council per-section rather than on the whole document, then do a final pass on section interactions.
- **Plans with extensive prior context:** tell the Question-Raiser to focus only on ambiguities not already resolved in the brief, to avoid noise.
- **Time-constrained reviews:** keep the council but cap each agent at its top 3 findings.

## Success criteria

A council run succeeded if:

1. At least one Critical or High finding was surfaced, OR the user explicitly confirmed the plan is sound after seeing the full council output.
2. Every open question shown to the user was either answered or explicitly deferred.
3. The plan file was updated in place, preserving the original structure, with a dated Council Review section appended.
4. The user can see — from the plan file alone — what changed, what was deferred, and why.
