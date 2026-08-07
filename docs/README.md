# Documentation

One folder per component, and the same four guides in each, so that knowing
where to look never depends on knowing the project.

| | Quickstart | Using it | Changing it | Its main tool |
|---|---|---|---|---|
| **[harness](harness/)** — the Rust agent, CLI, REPL and gateway | [quickstart](harness/quickstart-guide.md) | [user guide](harness/user-guide.md) | [code guide](harness/code-guide.md) | [CLI](harness/cli-guide.md) |
| **[desktop](desktop/)** — the MOMO WORK Tauri app | [install](desktop/momo-desktop-install.md) | [user guide](desktop/user-guide.md) | [code guide](desktop/code-guide.md) | [Tauri](desktop/tauri-guide.md) |
| **[web](web/)** — the Next.js UI | [quickstart](web/quickstart-guide.md) | [user guide](web/user-guide.md) | [code guide](web/code-guide.md) | [gateway API](web/gateway-api-guide.md) |
| **[brand](brand/)** — the artwork pipeline | [quickstart](brand/quickstart-guide.md) | [user guide](brand/user-guide.md) | [code guide](brand/code-guide.md) | [make-assets](brand/make-assets-guide.md) |

## Across all four

- **[project-landscape.md](project-landscape.md)** — what is in the repository, the
  module map, the dependency graph and its cycles. Start here if you are new to
  the code rather than to the product.

## Harness extras

The harness predates this layout and carries more than the four:
[LLM providers](harness/llm-provider-guide.md) ·
[OpenRouter models](harness/openrouter-model-guide.md) ·
[settings reference](harness/settings-quick-reference.md) ·
[settings architecture](harness/settings-architecture-explanation.md) ·
[provider improvements](harness/provider-improvement-suggestion.md) (a proposal, not a guide)

## Not guides

- **[spec/](spec/)** — `momo-worker.md` is the build plan; `momo-worker-scratchpad.md`
  is the live status and the wave log. `sds.md` and `memory-sidecar-sop.md` are
  design documents.
- **[prd/](prd/)** — the original product requirements. Historical.
- **[skills/](skills/)** — the handoff skill.

## Keeping these honest

The guides cite real paths and real `file:line` locations on purpose: a claim
you can check is worth more than one you have to trust. If you change something
a guide describes, the guide is part of the change.
