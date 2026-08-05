# Handoff Skill

Compact the current conversation into a handoff document for another agent to pick up and continue the work.

## When to use

Use when the user asks to hand off, switch agents, preserve context, or explicitly says `handoff`.

## Steps

1. Summarize the current conversation concisely
2. Identify key artifacts — reference specs, plans, ADRs, issues, commits, and diffs by path or URL (do not duplicate content)
3. Based on the user's argument about the next session's focus, suggest concrete next steps
4. Suggest which skills the next agent should invoke
5. Redact all sensitive information (API keys, passwords, PII)
6. Save the handoff document to:
   - macOS/Linux: `/tmp/handoff.md`
   - Windows: `%TEMP%\handoff.md`

## Output format

```markdown
# Handoff Document

## Context
Summary of the current conversation...

## Artifacts Created
- Plans and specifications
- Code changes
- Documentation

## Suggested Skills
- skill1: reason to use it
- skill2: reason to use it

## Next Steps
Based on the next session focus...

## Sensitive Information Redacted
[List of what was redacted]
```
