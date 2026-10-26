# Naming

The project is named Eileen. The owner chose the name on 2026-10-11, and the choice is final. The decision record is [D16 in DECISIONS.md](DECISIONS.md#d16-the-product-is-named-eileen).

## Where the name appears

| Thing | Name |
|---|---|
| Product | Eileen |
| Repository | `github.com/Sixteen1-6/eileen` |
| Binary and command | `eileen` (the `eileen-cli` crate), for example `eileen --version` |
| Environment variables | `EILEEN_` prefix, for example `EILEEN_LOCAL_ONLY` |
| Data directory | `%LOCALAPPDATA%\eileen` |
| MCP server names | `eileen-` prefix, for example `eileen-desktop` |

## Package registries

Checked against each registry's public API on 2026-10-11.

| Registry | `eileen` |
|---|---|
| PyPI | Free |
| crates.io | Free |
| npm | Taken by an unrelated Dokku command line tool, last published in 2022 |

## History

- The first codename was dropped because an NVIDIA hardware product line and three AI-agent companies already used it, and its wake word had only four phonemes.
- An earlier working name was used until 2026-10-11 and was never meant to ship: a funded startup in the same category already uses it, and it belongs to a commercial book and film franchise.
- The research brief vetted sixty candidates and ranked a shortlist with Ilmari at the top. The owner chose Eileen instead, which closes the shortlist. The vetting notes stay in [RESEARCH.md, section 8](RESEARCH.md#8-name).
- Neither earlier name appears in the repository's current files any more (owner answer 15 of 2026-10-20 in [OWNER-DIRECTION.md](OWNER-DIRECTION.md)); the house-style check of BUILD.md section 4b.3 (check 6) keeps them out.
