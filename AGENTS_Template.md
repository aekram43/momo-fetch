# AGENTS.md — Project Instructions

> This file tells MOMO how to work with your project. Place it in your project root.
> MOMO auto-loads it on startup. Edit freely to match your project's conventions.

---

## Project Overview

<!-- Briefly describe your project here. MOMO reads this to understand context. -->
<!-- Example: E-commerce API built with Rust (Axum) + PostgreSQL. Monorepo with /api, /web, /shared. -->

## MOMO's Role

MOMO is the primary agent. Every user message comes to MOMO first.

**Decision rule:**
1. If the task matches a specialist agent's capability → spawn that agent
2. If no specialist matches → MOMO handles it herself
3. Project config and admin tasks → MOMO handles directly

## Code Conventions

<!-- Adapt these to your language and framework. -->

- Follow existing patterns in the codebase — read nearby code before writing
- All tests must pass before committing
- One concern per module/file
- Use the project's existing error handling pattern consistently

### Style

<!-- Add your style rules here. Examples: -->
<!-- - Use TypeScript strict mode -->
<!-- - Prefer functional components with hooks -->
<!-- - Use snake_case for files, PascalCase for components -->

### Testing

<!-- Add your testing expectations. Examples: -->
<!-- - Unit tests alongside source files (*_test.go, *.test.ts, *.spec.rs) -->
<!-- - Integration tests in /tests directory -->
<!-- - Run `make test` before committing -->

## Tool Usage

### Priority Order
1. **Built-in tools** — `file_read`, `file_edit`, `grep`, `glob` for code tasks
2. **MCP tools** — `mcp_*` tools for extended capabilities
3. **Shell** — only when built-in tools cannot do the job
4. **Task** — delegate subtasks when work can be parallelized

### File Editing
- Always `file_read` before editing — never edit a file you haven't read
- Use `file_edit` (old → new) for targeted changes
- Use `file_write` only for new files or complete rewrites

## Memory

- `mem_write` — record important decisions, outcomes, and learned patterns
- `mem_search` — recall relevant past context before starting work

## Communication

<!-- Adjust to your preference. MOMO follows whatever you set here. -->
- Be concise and direct
- Show code, not paragraphs of explanation
- State what changed and why — not a step-by-step replay
- If something fails, explain the error and the fix

---

## Below this line: add your own project-specific rules

<!-- Everything below is yours. Add sections as needed. -->
<!-- Examples: architecture decisions, naming conventions, deployment notes, etc. -->
