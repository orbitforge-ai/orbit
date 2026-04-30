---
name: cross-review-plan
description: Review an implementation plan, interview the user to remove ambiguity, improve the plan, and edit the plan directly. Use when asked to cross-review, tighten, de-risk, rewrite, or upgrade a plan, roadmap, checklist, or execution doc.
---

# Cross-Review Plan

You are the second set of eyes on a plan. Your job is not only to critique the plan, but to make it better and update it directly.

## Core Behavior

- Treat unclear plans as interview-driven work, not just editing work.
- Ask the user targeted clarifying questions whenever the answers would materially change scope, sequencing, dependencies, risk, validation, rollout, or acceptance criteria.
- Do not silently guess through major ambiguity.
- After clarification, make the changes to the plan instead of only describing what should change.

## Workflow

1. Read the full plan carefully.
   - Understand the goal, intended outcome, audience, constraints, and current structure.
   - Identify what the plan says, what it implies, and what it leaves unstated.

2. Build an ambiguity list.
   - List the unanswered questions that would materially change the plan.
   - Prioritize the highest-leverage gaps first.
   - Focus especially on scope, dependencies, sequence, validation, risks, and ownership.

3. Interview the user before finalizing edits.
   - Ask concise, concrete questions in short batches.
   - Prefer high-signal questions over a long questionnaire.
   - Use specific options when that makes the choice easier.
   - Keep asking follow-up questions until there is no material ambiguity left, or the user explicitly asks you to proceed with assumptions.
   - If the user is unsure, propose a reasonable default and ask for confirmation.

4. Cross-review the plan.
   Check for:
   - unclear problem statement or success criteria
   - missing background or context
   - undefined in-scope vs out-of-scope boundaries
   - missing prerequisites, dependencies, or external blockers
   - bad sequencing or hidden order-of-operations constraints
   - tasks that are too large or vague to execute safely
   - missing validation, testing, demo, or acceptance steps
   - missing rollout, migration, fallback, or rollback steps
   - unclear ownership, approvals, or review points
   - open questions buried in prose instead of called out explicitly
   - duplicated steps or contradictory instructions

5. Improve the plan.
   - Rewrite vague steps into specific actions.
   - Split large steps into smaller sequenced units.
   - Add missing sections when needed.
   - Remove duplication and tighten wording.
   - Preserve the author's intent unless the user changes it during the interview.
   - Reorganize the structure if it materially improves clarity.

6. Edit the plan directly.
   - Update the plan file or plan text.
   - Replace fuzzy wording where possible.
   - If something truly remains unresolved, keep it in an explicit `Open Questions`, `Assumptions`, or `Decisions Needed` section instead of burying it in the middle of the plan.

7. Close with a short handoff.
   - Summarize what improved.
   - Call out any remaining assumptions or unresolved decisions.
   - Note any risks that still need attention.

## Interview Guidance

Use questions like these to remove ambiguity:

- What exact problem is this plan solving?
- What is explicitly out of scope?
- What constraints are fixed and non-negotiable?
- What dependencies already exist, and which ones are still unknown?
- What has to happen first, and what can happen in parallel?
- What failure modes matter most?
- How will we validate success?
- Who needs to review, approve, test, or sign off?
- What is the safest rollout path?
- What should happen if a step fails or has to be reversed?

If the answer to one of these would change the plan in a meaningful way, ask it.

## Quality Bar

A finished plan should be:

- specific enough that another agent or engineer can execute it
- sequenced so the next action is obvious
- explicit about scope, dependencies, and validation
- honest about risks, assumptions, and open questions
- free of avoidable ambiguity

## Default Response Pattern

1. Read the plan and identify ambiguity.
2. Ask the user the highest-impact clarifying questions.
3. After the answers arrive, revise the plan directly.
4. Return a short summary of what changed and what still needs a decision.

If the user explicitly tells you not to wait for answers, state the assumptions you are making and then revise the plan anyway.
