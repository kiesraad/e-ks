FROM mcr.microsoft.com/playwright:v1.65.0-noble@sha256:9beb546099e11f52d62e505537914870e01a67e0e6b00ae41da4775324e73dbe
WORKDIR /app
COPY playwright.config.ts /app/playwright.config.ts

# The image ships the browsers but not the test runner; install the runner at
# the exact version the image records so the two never drift apart.
# Edge is not part of the image and comes unpinned from Microsoft's apt repo,
# so it is only installed on request (INSTALL_EDGE=1).
ARG INSTALL_EDGE=
RUN version="$(node -p 'JSON.parse(require("fs").readFileSync("/ms-playwright/.docker-info")).driverVersion')" \
    && npm install --no-audit --no-fund "@playwright/test@$version" \
    && if [ -n "$INSTALL_EDGE" ]; then \
        npx playwright install --with-deps msedge && rm -rf /var/lib/apt/lists/*; \
    fi \
    && chmod a+w /app
USER pwuser

# Arguments to `docker compose run playwright` replace the default projects.
ENTRYPOINT ["npx", "playwright", "test", "--workers=3"]
CMD ["--project=chromium", "--project=firefox", "--project=webkit"]
