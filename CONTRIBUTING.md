# Contributing

Issues and pull requests are welcome. minSVG is a **Rust SVG optimizer**: rustc **1.83**, `cargo test`, ~2 MB MIT CLI.

The project author is [Gromsi](https://github.com/Gromsi). Use your own git name on commits. If you want Gromsi listed as author on a change, say so in the PR.

## Fork, branch, PR

1. [Fork](https://github.com/Gromsi/minSVG/fork) [Gromsi/minSVG](https://github.com/Gromsi/minSVG).
2. Clone your fork and point `upstream` at this repo:

   ```bash
   git clone https://github.com/<you>/minSVG.git
   cd minSVG
   git remote add upstream https://github.com/Gromsi/minSVG.git
   ```

3. Branch from latest `main`:

   ```bash
   git fetch upstream
   git checkout -b your-change upstream/main
   ```

4. Make the change. Keep the default CLI ~2 MB (`serve` and `mcp` stay feature-gated).
5. Run tests (below).
6. Push the branch and [open a pull request](https://github.com/Gromsi/minSVG/compare) against `Gromsi/minSVG` `main`.

One idea per PR when you can. Say what changed and how you tested it.

## Setup

**rustc 1.83+** (this repo pins `1.83.0` in `rust-toolchain.toml`) and a C compiler (`oxipng` / `libdeflater`).

```bash
rustc --version   # 1.83.x
cargo test
cargo test --features serve
cargo test --features mcp   # 388 passed on the last count
cd npm && npm test
```

CI on every push and PR runs `cargo test --locked` on rustc 1.83, then `--features mcp` and `npm test`.

## Bugs and ideas

- [Bug report](https://github.com/Gromsi/minSVG/issues/new?template=bug.yml) — command, rustc version, expected vs actual, and a **tiny** SVG when you can.
- [Feature request](https://github.com/Gromsi/minSVG/issues/new?template=feature.yml) — open an issue first if the change is large.

Do not paste huge SVGs into the issue body. Attach a file or a minimal fixture.

## Scope

Keep docs honest: Rust, ~25× on the filmed UI batches, ~2 MB CLI, `cargo test` **368**, landmines **9/9**. Do not claim crates.io or npmjs publication until that lands.

Leave animation-aware defaults on unless the change is specifically about `--no-animation-aware`.

## Code of conduct

Be kind. See [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).
