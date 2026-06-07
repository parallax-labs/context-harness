# Runbook Policy

This document defines how Context Harness uses **runbooks**. It applies to all documents in `docs/runbook/` and governs what a runbook captures, when to write one, how to structure it, and how it relates to the rest of the documentation hierarchy.

---

## 1. What a runbook is

A **runbook** is a **step-by-step operational procedure for a discrete task**. It tells an operator exactly what to do, in what order, and how to verify success — without requiring deep knowledge of the system's internals.

- **Procedural:** A runbook is a sequence of concrete steps. Each step is a command to run, a button to click, or a condition to verify. There is no ambiguity about what to do next.
- **Self-contained:** A runbook includes everything needed to complete the task: prerequisites, commands, expected outputs, verification steps, and troubleshooting for common failures. The operator should not need to consult other documents mid-procedure.
- **Task-scoped:** Each runbook covers one discrete task. "Deploy to Docker" is a runbook. "Everything about deployment" is a design doc. If a procedure requires more than one runbook, link them; do not merge them.

---

## 2. What a runbook is not

- **Not a design doc.** A design doc explores how a system works and why it was built that way. A runbook assumes the system exists and tells you how to operate it. If you need to explain architecture before the procedure makes sense, the explanation belongs in a design doc or spec, not inline in the runbook.
- **Not a spec.** A spec defines what the system MUST do. A runbook describes how an operator interacts with what the system already does.
- **Not a tutorial.** A tutorial teaches concepts and builds understanding. A runbook is optimized for execution — an operator follows the steps to achieve a specific outcome, not to learn.
- **Not a troubleshooting guide.** Each runbook includes a troubleshooting section for failures specific to that procedure. A standalone troubleshooting reference that covers errors across multiple procedures is acceptable as a runbook (e.g., "Common Errors") but should be clearly labeled.

---

## 3. When to write a runbook

Write a runbook when:

- **An operational task has more than two non-trivial steps.** If the procedure involves multiple commands, environment setup, or verification, capture it in a runbook.
- **The task is performed repeatedly.** Build, deploy, release, sync, backup — any recurring task deserves a runbook.
- **The task could be performed by someone who did not build the system.** If a team member, on-call engineer, or future contributor needs to execute this procedure, they should be able to follow the runbook without asking questions.
- **Failure is costly or hard to reverse.** Procedures that modify production data, publish releases, or change infrastructure should be documented to prevent mistakes.

Do not write a runbook for:

- One-off exploratory tasks (use a design doc or scratch notes).
- Defining system behavior (use a spec).
- Recording architectural decisions (use an ADR).

---

## 4. Runbook structure and required sections

Every runbook MUST include the following metadata and sections:

### Metadata

```
**Status:** Draft | Active | Deprecated
**Date:** YYYY-MM-DD
**Author:** name or handle
**Last Verified:** YYYY-MM-DD
```

The **Last Verified** date records when someone last executed the runbook end-to-end and confirmed it works. This is critical for trust — stale runbooks are worse than no runbooks.

### Required sections

| Section | Purpose |
|---------|---------|
| **Purpose** | One or two sentences: what this runbook accomplishes and when to use it. |
| **Prerequisites** | What must be true before starting: installed tools, environment variables, access permissions, prior runbooks that must be completed first. |
| **Steps** | Numbered, sequential steps. Each step includes the command or action, and the expected output or result. |
| **Verification** | How to confirm the procedure succeeded. Concrete checks, not "it should work." |
| **Troubleshooting** | Common failure modes specific to this procedure and how to resolve them. |

### Optional sections

- **Rollback:** How to undo the procedure if something goes wrong. Required for any runbook that modifies persistent state (deployments, database changes, releases).
- **Notes:** Additional context, tips, or gotchas that do not fit in the steps.
- **Related Runbooks:** Links to runbooks that are commonly performed before or after this one.

---

## 5. Writing effective steps

Steps are the core of a runbook. Follow these rules:

1. **Number every step.** Steps are executed in order. Numbering makes it easy to reference a specific step in conversation ("I'm stuck on step 4").

2. **One action per step.** Each step should be a single command or a single action. Do not combine "build and deploy" into one step.

3. **Show the command.** Use a fenced code block for every command. Include the working directory if it matters.

4. **Show the expected output.** After a command, show what the operator should see. If the output varies, describe the key indicators of success.

5. **Use conditional steps sparingly.** If a step only applies in certain conditions, use a clear "If ... then ..." structure. If there are many branches, consider splitting into separate runbooks.

6. **Never assume context.** Do not write "run the build command" — write `cargo build --release` with the exact flags.

---

## 6. Status lifecycle and review cadence

| Status | Meaning |
|--------|---------|
| **Draft** | The runbook is being written. Steps may be incomplete or unverified. |
| **Active** | The runbook is complete, verified, and in use. |
| **Deprecated** | The procedure is no longer needed (e.g., the system changed). The runbook remains for historical reference. |

### Review cadence

- Every runbook SHOULD be re-verified at least once per quarter.
- When a runbook is executed and found to be inaccurate, it MUST be updated immediately.
- The **Last Verified** date MUST be updated every time the runbook is successfully executed end-to-end.
- When the underlying system changes (new tooling, new deployment target, config changes), all affected runbooks MUST be reviewed and updated.

---

## 7. Relationship to other document types

| Layer | Purpose | Authority |
|-------|---------|-----------|
| **PRD** (`docs/prd/`) | What we build and why (user perspective) | Product intent |
| **ADR** (`docs/adr/`) | Why we chose a specific approach | Architectural rationale |
| **Spec** (`docs/spec/`) | Exactly how the system behaves | Behavioral contract |
| **Design** (`docs/design/`) | Exploration, planning, implementation guides | Not authoritative |
| **Runbook** (`docs/runbook/`) | Step-by-step operational procedures | Operational reference |

- A runbook MAY reference a **spec** for the behavior it operates on (e.g., "see [SPEC-0005](../spec/0005-usage-contract.md) for the full CLI reference").
- A runbook MAY reference a **design doc** for background context.
- A runbook SHOULD NOT duplicate content from specs or design docs. Link to them instead.
- When a new feature is delivered (PRD moves to Delivered), evaluate whether new runbooks are needed for operating that feature.

---

## 8. Summary

| Attribute | Runbook |
|-----------|---------|
| **Purpose** | Step-by-step procedure for a discrete operational task |
| **Authority** | Operational reference — not a behavioral contract |
| **Audience** | Operators, on-call engineers, contributors performing tasks |
| **When to write** | When an operational task has multiple steps and may be performed by someone unfamiliar with the internals |
| **Key rule** | Self-contained, concrete steps with verification; re-verify quarterly |
| **Lifecycle** | Draft → Active → Deprecated |

Runbooks are where documentation meets execution. They close the loop from "what does the system do" (specs) to "how do I operate the system" (runbooks). A project with good specs and no runbooks leaves operators guessing; a project with good runbooks gives every contributor confidence to build, deploy, and maintain the system.
