import { expect, test } from "@playwright/test";

import staleErrors from "../../../frontend/scripts/form-inputs/stale-errors";

const postalCodeField = `
  <p class="form-field form-field-sm">
    <label for="postal_code">Postcode</label>
    <input type="text" name="postal_code" id="postal_code" value="1234" />
    <span class="error">De postcode is ongeldig, gebruik het formaat 1234AB.</span>
  </p>
  <p class="form-field"><label for="elsewhere">Elders</label><input id="elsewhere" /></p>
`;

test.describe("stale-errors", () => {
  test("removes a server-rendered error once the edited field is left", async ({
    page,
  }) => {
    await page.setContent(postalCodeField);
    await page.evaluate(staleErrors);

    const error = page.locator("#postal_code ~ span.error");
    await expect(error).toBeVisible();

    // Typing alone leaves the message in place.
    await page.fill("#postal_code", "1234GG");
    await expect(error).toBeVisible();

    await page.locator("#elsewhere").focus();
    await expect(error).toHaveCount(0);
  });

  test("keeps the error when the field is left unedited", async ({ page }) => {
    await page.setContent(postalCodeField);
    await page.evaluate(staleErrors);

    const error = page.locator("#postal_code ~ span.error");

    await page.locator("#postal_code").focus();
    await page.locator("#elsewhere").focus();
    await expect(error).toBeVisible();
  });

  test("keeps the error when the submitted value is restored", async ({
    page,
  }) => {
    await page.setContent(postalCodeField);
    await page.evaluate(staleErrors);

    const error = page.locator("#postal_code ~ span.error");

    await page.fill("#postal_code", "1234GG");
    await page.fill("#postal_code", "1234");
    await page.locator("#elsewhere").focus();
    await expect(error).toBeVisible();
  });

  test("ignores checkboxes that belong to the field itself", async ({
    page,
  }) => {
    await page.setContent(`
      <p class="form-field">
        <label for="initials">Voorletters</label>
        <input type="text" name="initials" id="initials" value="H" />
        <label class="autoformat"><input type="checkbox" id="autoformat" checked /></label>
        <span class="error">Deze waarde is ongeldig.</span>
      </p>
      <p class="form-field"><label for="elsewhere">Elders</label><input id="elsewhere" /></p>
    `);
    await page.evaluate(staleErrors);

    const error = page.locator("#initials ~ span.error");

    await page.uncheck("#autoformat");
    await expect(error).toBeVisible();

    await page.fill("#initials", "H.J.");
    await page.locator("#elsewhere").focus();
    await expect(error).toHaveCount(0);
  });
});
