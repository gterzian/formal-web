# Skills

Agent skills for this repository live in `.agents/skills/<name>/`. Each
directory contains a `SKILL.md` with the full instructions and a self-contained
script in `scripts/`. Pi discovers these directories natively; other harnesses
can read `SKILL.md` directly and run the script with `node`.

`AGENTS.md` (section "Skills") lists each skill's trigger and command line.

| Skill | Path | Purpose |
| --- | --- | --- |
| `web-standards` | `.agents/skills/web-standards/` | Read spec text by anchor id, find ids, list reference sites, verify annotations |
| `readme-chain` | `.agents/skills/readme-chain/` | Collect the nested `README.md` chain for a path |
| `browser-debug` | `.agents/skills/browser-debug/` | Drive formal-web over CDP for live debugging |
| `startup-page-test` | `.agents/skills/startup-page-test/` | Run the startup-page test plan (explicit invocation only) |

Rebuild the committed bundles after editing a skill's `src/` (run `npm install`
once to get `esbuild` and `cheerio`):

```bash
npm run build:skills
```
