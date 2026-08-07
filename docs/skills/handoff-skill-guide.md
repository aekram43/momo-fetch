# Handoff Skill

## Overview

The **handoff** skill compacts the current conversation into a handoff document for another agent to pick up and continue the work.

## Purpose

This skill is useful when:
- You need to switch between different AI agents or sessions
- You want to preserve context across different conversations
- You're handing off work to another developer or AI agent
- You need to document the current state of a complex project

## Usage

Invoke the skill with:
```
/handoff <what the next session will be used for>
```

### Example Usage:
```
/handoff Continue debugging the authentication issue
/handoff Focus on implementing the new payment flow
/handoff Review the architecture and suggest improvements
```

## What It Does

The handoff skill:

1. **Summarizes the conversation**: Creates a concise summary of what has been discussed
2. **Identifies key artifacts**: References specs, plans, ADRs, issues, commits, and diffs
3. **Suggests next steps**: Based on the provided argument about the next session's focus
4. **Recommends skills**: Suggests which skills the next agent should invoke
5. **Protects sensitive info**: Redacts API keys, passwords, and PII

## Output

The handoff document is saved to:
- **macOS**: `/tmp/handoff.md`
- **Linux**: `/tmp/handoff.md`
- **Windows**: `%TEMP%\handoff.md`

## Document Structure

The generated handoff document includes:

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
Based on your argument about the next session focus...

## Sensitive Information Redacted
[List of what was redacted]
```

## Configuration

The skill is configured to:
- **Not invoke models**: `disable-model-invocation: true`
- **Require explicit invocation**: `allow_implicit_invocation: false`

## Integration

This skill integrates with the agent-harness system and respects all provider configurations set in your `.harness/settings.json`.

## See Also

- [Harness user guide § Skills](../harness/user-guide.md#9-skills)
- [Harness settings reference](../harness/settings-quick-reference.md)
