# Custom Command Templates

Create `.harness/commands/{name}.md` files to add your own slash commands.

## Quick Start

```bash
mkdir -p .harness/commands
```

Then create a markdown file for each command. The filename (without `.md`) becomes the command name.

## $ARG Placeholder

Use `$ARG` where the user's input should be inserted.

- `/review src/main.rs` → `$ARG` = `src/main.rs`
- `/explain` (no args) → `$ARG` = `` (empty)

## Examples

### /review — Code Review

`.harness/commands/review.md`:
```markdown
You are a senior code reviewer. Review the following code for:

1. Bugs and logic errors
2. Security vulnerabilities
3. Performance issues
4. Code style and readability

Provide actionable suggestions with specific fixes.

Code to review: $ARG
```

### /test — Generate Tests

`.harness/commands/test.md`:
```markdown
Write comprehensive unit tests for the following code.

Requirements:
- Cover happy path and edge cases
- Use descriptive test names
- Include error cases

Target: $ARG
```

### /fix — Debug & Fix

`.harness/commands/fix.md`:
```markdown
Analyze and fix the bug described below.

Steps:
1. Identify the root cause
2. Propose the minimal fix
3. Explain why the fix works

Bug description or error: $ARG
```

### /refactor — Refactor Code

`.harness/commands/refactor.md`:
```markdown
Refactor the following code to improve:

- Readability and clarity
- Error handling
- Performance (if applicable)

Do NOT change the external behavior or API.
Keep changes minimal and focused.

Target: $ARG
```

### /explain — Explain Code

`.harness/commands/explain.md`:
```markdown
Explain the following code in simple terms.

Cover:
- What it does (high level)
- How it works (step by step)
- Any non-obvious patterns or trade-offs

Target: $ARG
```

### /commit — Generate Commit Message

`.harness/commands/commit.md`:
```markdown
Look at the current git diff and generate a clear, concise commit message.

Format: type(scope): description

Types: feat, fix, refactor, docs, test, chore

Context: $ARG
```

### /security — Security Audit

`.harness/commands/security.md`:
```markdown
Perform a security audit on the following code.

Check for:
- OWASP Top 10 vulnerabilities
- Input validation issues
- Authentication/authorization flaws
- Sensitive data exposure
- Injection risks

Target: $ARG
```

## Tips

- The first line of the file appears as the description in `/help`
- Commands are discovered at invocation time — no restart needed
- Use `<!-- comments -->` to hide notes from the help description
- Keep prompts focused — they are sent as user messages to the LLM
