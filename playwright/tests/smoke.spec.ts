import { expect } from "@playwright/test";
import { test } from "./fixtures.ts";

test("homepage loads", async ({ login: page }) => {
  await page.goto("/");
  await expect(page).toHaveTitle(/^Overzicht - Kandidaatstelling/);
});
