FROM mcr.microsoft.com/playwright:v1.58.2-noble
WORKDIR /app
COPY package*.json /app/
COPY playwright.config.ts /app/playwright.config.ts
RUN npm ci

# Arguments to `docker compose run playwright` replace the default projects.
ENTRYPOINT ["npx", "playwright", "test", "--workers=3"]
CMD ["--project=chromium", "--project=firefox", "--project=webkit"]
