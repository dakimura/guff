# License FAQ (GPL-3.0)

guff is licensed under the **GNU General Public License v3.0** ([LICENSE](../LICENSE)). Analyzer ports retain their upstream attributions in [THIRD_PARTY_LICENSES.md](../THIRD_PARTY_LICENSES.md).

This is not legal advice. When in doubt, ask your counsel.

## Using the `guff` CLI on my proprietary Go project

**Typically fine.** Running guff as a separate program (local shell, CI Action, Docker) to lint your code does not require you to release your Go application under the GPL. Your source stays under whatever license you choose.

Same model as running other GPL tools in a build pipeline.

## Shipping guff inside my product / linking as a library

If you **modify guff** and distribute the modified binary, or **statically/dynamically link** guff’s libraries into a proprietary application you distribute, GPL-3.0 obligations likely apply (offer source, keep GPL notices, etc.).

Prefer invoking the released `guff` binary as an external tool if you need to avoid combining licenses.

## CI / SaaS that only executes guff on customer code

Executing an unmodified guff binary as a service is usually treated like running the CLI; distributing a modified guff to customers is not. Read GPL-3.0 §§ for conveyance vs. mere aggregation.

## Why GPL?

guff incorporates substantial ports of existing Go analyzers and aims to stay honest about copyleft upstream DNA. See third-party notices for per-component licenses.

## Dual licensing

Not offered today. If your org needs a different license for embedding, open a discussion on GitHub.
