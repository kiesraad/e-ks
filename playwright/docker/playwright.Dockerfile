FROM mcr.microsoft.com/playwright:v1.65.0-noble
WORKDIR /app
COPY playwright.config.ts /app/playwright.config.ts

# The image ships the browsers but not the test runner; install the runner at
# the exact version the image records so the two never drift apart.
# Edge is not part of the image; `playwright install` adds Microsoft's apt repo
# and installs it.
RUN version="$(node -p 'JSON.parse(require("fs").readFileSync("/ms-playwright/.docker-info")).driverVersion')" \
    && npm install --no-audit --no-fund "@playwright/test@$version" \
    && npx playwright install --with-deps msedge \
    && rm -rf /var/lib/apt/lists/* \
    && chmod a+w /app
USER pwuser

# Arguments to `docker compose run playwright` replace the default projects.
ENTRYPOINT ["npx", "playwright", "test", "--workers=3"]
CMD ["--project=chromium", "--project=firefox", "--project=webkit"]
