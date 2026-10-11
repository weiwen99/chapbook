# Domain Docs

This repo uses a single-context layout: `GLOSSARY.md` at the repo root and ADRs under `docs/adr/`.

## Before exploring, read these

- `GLOSSARY.md` at the repo root, if it exists.
- ADRs under `docs/adr/` that touch the area you're about to work in.

If these files don't exist, proceed silently. Don't flag their absence or suggest creating them upfront. `/domain-modeling`, reached via `/grill-with-docs` and `/improve-codebase-architecture`, creates them lazily when terms or decisions get resolved.

## Use the glossary's vocabulary

When naming a domain concept in an issue title, refactor proposal, hypothesis, or test name, use the term defined in `GLOSSARY.md`. Don't drift to synonyms the glossary explicitly avoids.

If a concept isn't in the glossary, reconsider whether the language belongs to this project or note a real gap for `/domain-modeling`.

## Flag ADR conflicts

If your output contradicts an existing ADR, surface the conflict explicitly: identify the ADR and explain why its decision is worth reopening.
