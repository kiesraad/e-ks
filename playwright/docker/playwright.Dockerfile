FROM mcr.microsoft.com/playwright:v1.64.1-noble
WORKDIR /app
COPY playwright.config.ts /app/playwright.config.ts

# The image ships the browsers but not the test runner; install the runner at
# the exact version the image records so the two never drift apart.
RUN npm install --no-audit --no-fund \
    "@playwright/test@$(node -p 'require("/ms-playwright/.docker-info").driverVersion')"

# Edge is not part of the image; this adds Microsoft's apt repo and installs it.
RUN npx playwright install --with-deps msedge && rm -rf /var/lib/apt/lists/*

RUN chmod a+w /app
USER pwuser

# Arguments to `docker compose run playwright` replace the default projects.
ENTRYPOINT ["npx", "playwright", "test", "--workers=3"]
CMD ["--project=chromium", "--project=firefox", "--project=webkit"]
