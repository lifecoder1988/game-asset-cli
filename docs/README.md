# game-asset-cli docs

`game-asset-cli` is a stateless command-line toolkit for generating and
post-processing game assets.

It is not a project manager. It does not create projects, keep job state, start a
runner, maintain a database, or own an asset pipeline. Every command reads
explicit inputs and writes explicit outputs.

## Design Position

- Image generation wraps the local Codex CLI.
- Background music generation uses the MiniMax music API.
- Short sound effects are synthesized locally first, with a provider interface so
  they can later move to a model backend.
- Rust is the primary implementation language.
- Batch mode is allowed, but the batch file is an explicit input spec, not CLI
  state.

## Document Map

- [spec.md](spec.md) - product scope, stateless contract, command surface, exit
  behavior.
- [technical-design.md](technical-design.md) - Rust architecture, modules,
  traits, IO guarantees, testing strategy.
- [providers.md](providers.md) - Codex image backend, MiniMax music backend,
  local SFX backend, and provider replacement rules.
- [batch-spec.md](batch-spec.md) - versioned YAML batch format for repeatable
  multi-asset generation.
- [roadmap.md](roadmap.md) - implementation milestones and non-goals.

## External References

Checked on 2026-06-19:

- Codex CLI local help: `codex --help`, `codex exec --help`.
- Codex manual: local cached copy from
  `https://developers.openai.com/codex/codex-manual.md`.
- MiniMax official docs:
  - `https://platform.minimax.io/docs/api-reference/api-overview`
  - `https://platform.minimax.io/docs/api-reference/music-generation`
  - `https://platform.minimax.io/docs/guides/models-intro`

