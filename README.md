# e-KS

Het elektronisch Kandidaatstellingssysteem (e-KS) is software die politieke partijen en centraal stembureaus ondersteunt bij de kandidaatstellingsprocedure.

Om namelijk te kunnen deelnemen aan een verkiezing moet een politieke groepering aangeven met welke kandidaten ze mee wil doen. Hiervoor moeten ze verschillende documenten inleveren bij het centraal stembureau, dit wordt gedaan op de dag van kandidaatstelling, onderdeel van de kandidaatstellingsprocedure. 

e-KS is een webapplicatie waarmee de Kiesraad de huidige kandidaatstellingsprocedure op een eerlijke, transparante en controleerbare manier wil moderniseren. Het nieuwe systeem zal op termijn de huidige ondersteunende software (OSV2020-PP en OSV2020-KS) vervangen. 

## Requirements

De kandidaatstellingsprocedure is verankerd in de [Kieswet](https://wetten.overheid.nl/BWBR0004627/2025-08-01).

Een overzicht van het huidige proces en e-KS is te lezen in [deze presentatie](https://github.com/user-attachments/files/24053768/e-KS-Proces.pdf).

Het papieren proces is op dit moment leidend. e-KS helpt om de juiste documenten met de juiste gegevens te genereren. 
Belangrijke stukken of [formulieren voor de kandidaatstellingsprocedure](https://www.kiesraad.nl/verkiezingen/eerste-kamer/kandidaatstelling/stukken-kandidaatstelling) zijn:

- [Kandidatenlijst H1](https://www.rijksoverheid.nl/onderwerpen/verkiezingen/documenten/publicaties/2020/12/15/model-h-1-kandidatenlijst)
- [Instemmingsverklaring H9](https://www.rijksoverheid.nl/onderwerpen/verkiezingen/documenten/publicaties/2020/12/15/model-h-9-instemmingsverklaring)
- [Machtiging om aanduiding boven lijst te plaatsen H3-1](https://www.rijksoverheid.nl/documenten/publicaties/2020/12/15/model-h-3-1-machtiging-om-aanduiding-boven-kandidatenlijst-te-plaatsen)
- [Samenvoeging aanduidingen H3-2](https://www.rijksoverheid.nl/onderwerpen/verkiezingen/documenten/publicaties/2020/12/15/model-h-3-2-machtiging-om-samengevoegde-aanduiding-boven-kandidatenlijst-te-plaatsen)
- [Ondersteuningsverklaringen H4](https://www.rijksoverheid.nl/onderwerpen/verkiezingen/documenten/publicaties/2021/08/19/model-h-4-ondersteuningsverklaring)

## Kwaliteit waarborgen

Kwaliteit is een integraal onderdeel van het ontwikkelproces binnen het e-KS-team. Het is niet de laatste stap in het proces, maar is van begin tot eind volledig geïntegreerd in de ontwikkeling. Meer informatie over de werkwijze van het e-KS team is [hier](docs/werkwijze-kwaliteit-waarborgen.md) te vinden.

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

## Over de Kiesraad

De Kiesraad is de onafhankelijke autoriteit in Nederland op het gebied van verkiezingen. De missie van de Kiesraad is dat iedereen de uitslag van de verkiezingen kan vertrouwen.

Meer informatie over de Kiesraad en de verkiezingen is te vinden op onze [GitHub organisatie-pagina](https://github.com/kiesraad) en op www.kiesraad.nl
