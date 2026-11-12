# Playwright Tests

End-to-end tests for the application. They run inside the official Playwright
docker image, so there is nothing to install locally: no Node, npm or browsers.

## Running tests

From the repository root:

```bash
bin/test                                   # chromium, firefox and webkit
bin/test --project=edge                    # one browser (chromium, firefox, webkit, edge)
bin/test --project=firefox tests/csb-brp-check.spec.ts
```

`bin/test` builds the frontend and the `eks` binary, starts the application and
the BRP mock in docker, and runs the suite against them. Arguments are passed
to `playwright test`. Edge is only installed in the image when `--project=edge`
is passed (or `INSTALL_EDGE=1` is set). The HTML report lands in `playwright/playwright-report`.

## UI mode in development

With the dev server running (`bin/dev`, port 3000), start the Playwright UI
from the container and open it at http://localhost:8080:

```bash
cd playwright
docker compose run --rm --no-deps -p 8080:8080 \
  -e PLAYWRIGHT_TEST_BASE_URL=http://host.docker.internal:3000 \
  playwright --ui --ui-host=0.0.0.0 --ui-port=8080
```

`--no-deps` skips the dockerised application and BRP mock; the tests run
against your dev server instead. Edited tests are picked up live, since
`tests` is mounted into the container.

## Updating Playwright

Bump the image tag in `docker/playwright.Dockerfile`; the test runner is
installed at the version the image records, so it follows the tag.

## Notes

- Tests live in `playwright/tests` and are linted with `bin/biome`.
- Base URL: set `PLAYWRIGHT_TEST_BASE_URL` (defaults to `http://localhost:3000`).
