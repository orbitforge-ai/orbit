---
name: code-review
description: Review code changes for bugs, security issues, performance problems, and style. Use when asked to review code, a diff, a PR, or when checking code quality.
---

# Code Review

## Process

1. **Understand the change**: Read the code or diff to understand what was changed and why.
2. **Check correctness**: Look for logic errors, off-by-one bugs, null/undefined handling, and incorrect assumptions.
3. **Security scan**: Check for injection vulnerabilities (SQL, XSS, command), hardcoded secrets, improper auth checks, and unsafe deserialization.
4. **Performance check**: Look for N+1 queries, unnecessary allocations, missing indexes, unbounded loops, and blocking calls in async contexts.
5. **Error handling**: Verify errors are caught, logged, and surfaced appropriately. Check for swallowed errors and missing edge cases.
6. **Readability**: Assess naming, structure, and whether the code is self-documenting. Flag overly clever code.

## Output Format

Organize findings by severity:

### Critical
Issues that will cause bugs, data loss, or security vulnerabilities in production.

### Warning
Issues that could cause problems under certain conditions or indicate code smell.

### Suggestion
Improvements to readability, performance, or maintainability that aren't urgent.

## Gotchas

- Don't flag style preferences unless they impact readability. Focus on substance.
- If reviewing a diff, consider context that isn't shown — the change may rely on code outside the visible range.
- When unsure about intent, note the ambiguity rather than assuming it's a bug.
- Praise good patterns when you see them — reviews shouldn't be exclusively negative.
