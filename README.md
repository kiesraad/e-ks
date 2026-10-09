**Let op: dit project bevindt zich momenteel in een opstartfase. Documentatie en code zullen onvolledig en soms incorrect zijn.**

# e-KS

Om te kunnen deelnemen aan een verkiezing moet een politieke groepering aangeven met welke kandidaten ze mee wil doen. Hiervoor moeten ze verschillende documenten inleveren bij het centraal stembureau. Dit heet de kandidaatstellingsprocedure.

e-KS staat voor het elektronisch Kandidaatstellingssysteem: een webapplicatie waarmee de Kiesraad de huidige kandidaatstellingsprocedure op een eerlijke, transparante en controleerbare manier wil moderniseren. Het nieuwe systeem zal op termijn de huidige ondersteunende software (OSV2020-PP en OSV2020-KS) vervangen.

## Requirements

De kandidaatstellingsprocedure is verankerd in de [Kieswet](https://wetten.overheid.nl/BWBR0004627/2025-08-01).

Een overzicht van het huidige proces en e-KS is te lezen in [deze presentatie](https://github.com/user-attachments/files/24053768/e-KS-Proces.pdf).

Belangrijke stukken of [formulieren voor de kandidaatstellingsprocedure](https://www.kiesraad.nl/verkiezingen/eerste-kamer/kandidaatstelling/stukken-kandidaatstelling) zijn:

- [Kandidatenlijst H1](https://www.rijksoverheid.nl/onderwerpen/verkiezingen/documenten/publicaties/2020/12/15/model-h-1-kandidatenlijst)
- [Instemmingsverklaring H9](https://www.rijksoverheid.nl/onderwerpen/verkiezingen/documenten/publicaties/2020/12/15/model-h-9-instemmingsverklaring)
- [Machtiging om aanduiding boven lijst te plaatsen H3-1](https://www.rijksoverheid.nl/documenten/publicaties/2020/12/15/model-h-3-1-machtiging-om-aanduiding-boven-kandidatenlijst-te-plaatsen)
- [Samenvoeging aanduidingen H3-2](https://www.rijksoverheid.nl/onderwerpen/verkiezingen/documenten/publicaties/2020/12/15/model-h-3-2-machtiging-om-samengevoegde-aanduiding-boven-kandidatenlijst-te-plaatsen)
- [Ondersteuningsverklaringen H4](https://www.rijksoverheid.nl/onderwerpen/verkiezingen/documenten/publicaties/2021/08/19/model-h-4-ondersteuningsverklaring)

## Technische architectuur

Een overzicht van de voorgestelde technische afwegingen staat in [deze presentatie](https://github.com/user-attachments/files/24053801/e-KS-PSA.pdf).

Een diepere duik in de technische architectuur is te vinden in [deze documentatie](docs/code-architecture.md).

## Development setup

1) Install prerequisites:

- [Rust](https://www.rust-lang.org/tools/install)
- [Docker](https://docs.docker.com/get-docker/)

2) Build and download development tools:

```bash
bin/init
```

3) Start the development environment (postgres, esbuild, cargo watch, etc.):

```bash
bin/dev
```

## Faster local builds

Each distinct cargo configuration (features, rustflags, linker) gets its own
copy of the dependency tree in `target/`, so keep them identical: `bin/dev`
builds with `--features development`, use the same flag in the terminal. Two
per-machine settings cannot live in the repository:

1. **rust-analyzer**: same features, own build directory, no `linkedProjects`
   for the member crates. In `.vscode/settings.json` (gitignored):

   ```json
   {
       "rust-analyzer.cargo.features": ["development"],
       "rust-analyzer.cargo.targetDir": true
   }
   ```

2. **Linker**: link with [wild](https://github.com/davidlattimore/wild) via
   clang (`cargo install wild-linker`). In `.cargo/config.toml` (gitignored):

   ```toml
   [target.x86_64-unknown-linux-gnu]
   linker = "clang"
   rustflags = ["-C", "link-arg=--ld-path=wild"]
   ```

## Development tools

- `bin/esbuild`: transpile and bundle Typescript and CSS, also services frontend assets in development
- `bin/biome`: format and lint Typescript
- `bin/setup`: download tools, setup database, load fixtures, etc.
- `bin/dev`: start development environment (postgres, esbuild, cargo watch, etc.)
- `bin/test`: run integration tests (playwright) in a docker conmtainer
- `bin/init`: build and download development tools
- `bin/check`: run linter, formatters and Rust tests
- `bin/build`: build backend and frontend for production
- `bin/update_locales`: update locale files based on used keys in the codebase

## Playwright tests

Playwright lives in `playwright`. See `playwright/README.md` for setup and run instructions.
