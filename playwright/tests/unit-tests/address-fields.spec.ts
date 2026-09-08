import { expect, type Page, test } from "@playwright/test";

import localitySuggestions from "../../../frontend/scripts/form-inputs/locality-suggestions";
import addressLookup from "../../../frontend/scripts/form-inputs/lookup";
import staleErrors from "../../../frontend/scripts/form-inputs/stale-errors";

// `address_fields.html` after a submit rejected for a malformed postal code,
// for a stored address that is not in the BAG.
const addressFields = `
  <form class="form">
    <div class="form-row">
      <p class="form-field form-field-sm warning">
        <label for="postal_code">Postcode</label>
        <input type="text" name="postal_code" id="postal_code" value="1234" />
        <span class="error">De postcode is ongeldig, gebruik het formaat 1234AB.</span>
        <span class="warning" id="unknown-address">Adres niet gevonden in de BAG</span>
      </p>
      <p class="form-field form-field-sm warning">
        <label for="house_number">Huisnummer</label>
        <input type="text" name="house_number" id="house_number" value="12" />
      </p>
      <p class="form-field form-field-sm">
        <label for="house_number_addition">Huisnummer toevoeging</label>
        <input type="text" name="house_number_addition" id="house_number_addition" value="" />
      </p>
    </div>
    <div class="form-row">
      <p class="form-field">
        <label for="street_name">Straatnaam</label>
        <input type="text" name="street_name" id="street_name" value="" />
      </p>
      <p class="form-field">
        <label for="locality">Woonplaats</label>
        <input type="text" name="locality" id="locality" autocomplete="off" value="" />
        <span id="locality-suggestion" class="suggestion hidden">
          Bedoelde u <button type="button" id="locality-suggestion-name"></button>?
        </span>
      </p>
    </div>
  </form>
`;

// Served from a real origin so the relative `/lookup` and `/suggest` requests
// resolve. The stubbed BAG knows `1012JS 1` only.
async function setupPage(page: Page) {
  await page.route("**/lookup*", (route) => {
    const url = new URL(route.request().url());
    const known = url.searchParams.get("pc") === "1012JS";

    return known
      ? route.fulfill({ json: { pr: "Dam", wp: "Amsterdam" } })
      : route.fulfill({ status: 404, json: { error: "address not found" } });
  });
  await page.route("**/suggest*", (route) => route.fulfill({ json: [] }));
  await page.route("**/form", (route) =>
    route.fulfill({ contentType: "text/html", body: addressFields }),
  );

  await page.goto("http://address.test/form");

  // Same order as `frontend/index.ts`.
  await page.evaluate(addressLookup);
  await page.evaluate(localitySuggestions);
  await page.evaluate(staleErrors);
}

const fieldClass = (page: Page, id: string) =>
  page
    .locator(`#${id}`)
    .evaluate((input) => input.closest(".form-field")?.className ?? "");

test.describe("address fields", () => {
  test("a corrected postal code drops the stale error and runs the lookup", async ({
    page,
  }) => {
    await setupPage(page);

    const error = page.locator("#postal_code ~ span.error");
    await expect(error).toBeVisible();

    await page.fill("#postal_code", "1012JS");
    // Still shown while the field is being edited.
    await expect(error).toBeVisible();

    // Leaving the field drops the message, before the lookup even resolves.
    await page.fill("#house_number", "1");
    await expect(error).toHaveCount(0);
    await page.locator("#house_number_addition").click();

    // The lookup still auto-fills and marks the address as found.
    await expect(page.locator("#street_name")).toHaveValue("Dam");
    await expect(page.locator("#locality")).toHaveValue("Amsterdam");
    await expect(page.locator("#unknown-address")).toHaveClass(/hidden/);
    expect(await fieldClass(page, "postal_code")).toContain("success");
    expect(await fieldClass(page, "postal_code")).not.toContain("warning");
    await expect(error).toHaveCount(0);
  });

  test("dropping the stale error leaves the BAG warning to the lookup", async ({
    page,
  }) => {
    await setupPage(page);

    const error = page.locator("#postal_code ~ span.error");

    await page.fill("#postal_code", "9999ZZ");
    await page.locator("#house_number_addition").click();

    // The stale message is gone, but the address is still flagged as unknown.
    await expect(error).toHaveCount(0);
    await expect(page.locator("#unknown-address")).not.toHaveClass(/hidden/);
    expect(await fieldClass(page, "postal_code")).toContain("warning");
    expect(await fieldClass(page, "house_number")).toContain("warning");
  });

  test("accepting a locality suggestion still clears the locality warning", async ({
    page,
  }) => {
    await setupPage(page);
    await page.route("**/suggest*", (route) =>
      route.fulfill({ json: ["Amsterdam"] }),
    );

    await page.fill("#locality", "Amsterdaa");
    await page.locator("#street_name").click();
    await expect(page.locator("#locality-suggestion")).not.toHaveClass(
      /hidden/,
    );
    expect(await fieldClass(page, "locality")).toContain("warning");

    await page.click("#locality-suggestion-name");
    await expect(page.locator("#locality")).toHaveValue("Amsterdam");
    expect(await fieldClass(page, "locality")).not.toContain("warning");
  });
});
