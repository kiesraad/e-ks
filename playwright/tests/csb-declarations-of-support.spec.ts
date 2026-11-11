import { expect } from "@playwright/test";
import { test } from "./fixtures.ts";
import { CsbOmissionsDeclarationsOfSupportPage } from "./pages/csb/csbOmissionsDeclarationsOfSupport.ts";
import { CsbPoliticalGroupPage } from "./pages/csb/csbPoliticalGroupPage.ts";

test.describe("add omissions for declarations of support", async () => {
  test("for single list", async ({ csbImport }) => {
    const { page, groupName } = csbImport;
    const politicalGroupPage = new CsbPoliticalGroupPage(page);
    const omissionsPage = new CsbOmissionsDeclarationsOfSupportPage(page);

    await politicalGroupPage.selectedGroup(groupName);

    // Add each type of omission, verify and then remove
    const omissions = [
      {
        button: omissionsPage.buttonMissing,
        text: "Voor één kieskring ontbreken ondersteuningsverklaringen",
        resolvable: true,
      },
      {
        button: omissionsPage.buttonDeclarationsNotValid,
        text: "Ondersteuningsverklaringen komen niet overeen met kandidatenlijst",
        resolvable: true,
      },
    ];

    for (const { button, text, resolvable } of omissions) {
      await politicalGroupPage.linkSupportDeclarations.click();
      await page.waitForURL(/\/omission\//);
      await expect(omissionsPage.headerDeclarationsOfSupport).toBeVisible();

      await page.getByRole("checkbox", { name: "1. Groningen" }).check();
      await button.click();
      if (button === omissionsPage.buttonMissing) {
        await expect(omissionsPage.textfieldLetter).toHaveValue(/Groningen/);
        await expect(omissionsPage.textfieldDescription).not.toHaveValue(
          /Groningen/,
        );
      }
      if (resolvable) {
        await expect(omissionsPage.checkboxRecoverable).toBeChecked();
        await omissionsPage.textfieldLetter.fill("Testtoevoeging");
      }
      await omissionsPage.buttonAddAndClose.click();
      await expect(page.locator("form.overlay")).toBeHidden();
      await politicalGroupPage.linkSupportDeclarationsOverview.click();
      await page.waitForURL(/\/omission\//);
      await expect(page.getByText(text)).toBeVisible();
      if (resolvable) {
        await expect(page.getByText("Testtoevoeging")).toBeVisible();
        await expect(page.getByText("Herstelbaar")).toBeVisible();
      } else {
        await expect(
          page.getByText("Onherstelbaar", { exact: true }),
        ).toBeVisible();
      }
      await expect(page.getByText("Groningen").first()).toBeVisible();
      await omissionsPage.clickRemoveOmission();
      await expect(
        page.getByText("Er zijn nog geen verzuimen toegevoegd."),
      ).toBeVisible();
      await omissionsPage.linkClose.click();
    }
  });

  test("for multiple lists", async ({ csbImport }) => {
    const { page, groupName } = csbImport;
    const politicalGroupPage = new CsbPoliticalGroupPage(page);
    const omissionsPage = new CsbOmissionsDeclarationsOfSupportPage(page);

    await politicalGroupPage.selectedGroup(groupName);

    // Add each type of omission, verify and then remove
    const omissions = [
      {
        button: omissionsPage.buttonMissing,
        text: "Voor meerdere kieskringen ontbreken ondersteuningsverklaringen",
        resolvable: true,
      },
      {
        button: omissionsPage.buttonDeclarationsNotValid,
        text: "Ondersteuningsverklaringen komen niet overeen met kandidatenlijst",
        resolvable: true,
      },
    ];

    for (const { button, text, resolvable } of omissions) {
      await politicalGroupPage.linkSupportDeclarations.click();
      await page.waitForURL(/\/omission\//);
      await expect(omissionsPage.headerDeclarationsOfSupport).toBeVisible();

      await page.getByRole("checkbox", { name: "1. Groningen" }).check();
      await page.getByRole("checkbox", { name: "2. Fryslân" }).check();
      await button.click();
      if (button === omissionsPage.buttonMissing) {
        await expect(omissionsPage.textfieldLetter).toHaveValue(
          /Groningen en Fryslân/,
        );
        await expect(omissionsPage.textfieldDescription).not.toHaveValue(
          /Groningen/,
        );
      }
      if (resolvable) {
        await expect(omissionsPage.checkboxRecoverable).toBeChecked();
        await omissionsPage.textfieldLetter.fill("Testtoevoeging");
      }
      await omissionsPage.buttonAddAndClose.click();
      await expect(page.locator("form.overlay")).toBeHidden();
      await politicalGroupPage.linkSupportDeclarationsOverview.click();
      await page.waitForURL(/\/omission\//);
      await expect(page.getByText(text)).toBeVisible();
      if (resolvable) {
        await expect(page.getByText("Testtoevoeging")).toBeVisible();
        await expect(page.getByText("Herstelbaar")).toBeVisible();
      } else {
        await expect(
          page.getByText("Onherstelbaar", { exact: true }),
        ).toBeVisible();
      }
      await expect(page.getByText("Groningen").first()).toBeVisible();
      await expect(page.getByText("Fryslân").first()).toBeVisible();
      await omissionsPage.clickRemoveOmission();
      await expect(
        page.getByText("Er zijn nog geen verzuimen toegevoegd."),
      ).toBeVisible();
      await omissionsPage.linkClose.click();
    }
  });

  test("for all lists", async ({ csbImport }) => {
    const { page, groupName } = csbImport;
    const politicalGroupPage = new CsbPoliticalGroupPage(page);
    const omissionsPage = new CsbOmissionsDeclarationsOfSupportPage(page);

    await politicalGroupPage.selectedGroup(groupName);

    // Add each type of omission, verify and then remove
    const omissions = [
      {
        button: omissionsPage.buttonMissing,
        text: "Voor alle kieskringen ontbreken ondersteuningsverklaringen",
        resolvable: true,
      },
      {
        button: omissionsPage.buttonDeclarationsNotValid,
        text: "Ondersteuningsverklaringen komen niet overeen met kandidatenlijst",
        resolvable: true,
      },
    ];

    for (const { button, text, resolvable } of omissions) {
      await politicalGroupPage.linkSupportDeclarations.click();
      await page.waitForURL(/\/omission\//);
      await expect(omissionsPage.headerDeclarationsOfSupport).toBeVisible();

      await page
        .getByRole("checkbox", { name: "selecteer alle kieskringen" })
        .check();
      await button.click();
      if (resolvable) {
        await expect(omissionsPage.checkboxRecoverable).toBeChecked();
        await omissionsPage.textfieldLetter.fill("Testtoevoeging");
      }
      await omissionsPage.buttonAddAndClose.click();
      await expect(page.locator("form.overlay")).toBeHidden();
      await politicalGroupPage.linkSupportDeclarationsOverview.click();
      await page.waitForURL(/\/omission\//);
      await expect(page.getByText(text)).toBeVisible();
      if (resolvable) {
        await expect(page.getByText("Testtoevoeging")).toBeVisible();
        await expect(page.getByText("Herstelbaar")).toBeVisible();
      } else {
        await expect(
          page.getByText("Onherstelbaar", { exact: true }),
        ).toBeVisible();
      }
      await expect(
        page.getByText(
          "1 (Groningen), 2 (Fryslân), 3 (Drenthe), 4 (Overijssel), 5 (Flevoland), 6 (Gelderland), 7 (Utrecht)",
        ),
      ).toBeVisible();
      await omissionsPage.clickRemoveOmission();
      await expect(
        page.getByText("Er zijn nog geen verzuimen toegevoegd."),
      ).toBeVisible();
      await omissionsPage.linkClose.click();
    }
  });

  test("missing preset follows the district selection", async ({
    csbImport,
  }) => {
    const { page, groupName } = csbImport;
    const politicalGroupPage = new CsbPoliticalGroupPage(page);
    const omissionsPage = new CsbOmissionsDeclarationsOfSupportPage(page);

    await politicalGroupPage.selectedGroup(groupName);
    await politicalGroupPage.linkSupportDeclarations.click();
    await page.waitForURL(/\/omission\//);

    const groningen = page.getByRole("checkbox", { name: "1. Groningen" });
    const fryslan = page.getByRole("checkbox", { name: "2. Fryslân" });

    await groningen.check();
    await omissionsPage.buttonMissing.click();
    await expect(omissionsPage.textfieldTitle).toHaveValue(
      "Voor één kieskring ontbreken ondersteuningsverklaringen",
    );
    await expect(omissionsPage.textfieldLetter).toHaveValue(
      /kieskring Groningen\./,
    );

    await fryslan.check();
    await expect(omissionsPage.textfieldTitle).toHaveValue(
      "Voor meerdere kieskringen ontbreken ondersteuningsverklaringen",
    );
    await expect(omissionsPage.textfieldLetter).toHaveValue(
      /kieskringen Groningen en Fryslân\./,
    );

    await omissionsPage.checkboxAllLists.check();
    await expect(omissionsPage.textfieldTitle).toHaveValue(
      "Voor alle kieskringen ontbreken ondersteuningsverklaringen",
    );
    await expect(omissionsPage.textfieldLetter).not.toHaveValue(/Groningen/);

    // A manually edited text is left alone on a new selection
    await omissionsPage.textfieldLetter.fill("Eigen tekst");
    await fryslan.uncheck();
    await expect(omissionsPage.textfieldTitle).toHaveValue(
      "Voor meerdere kieskringen ontbreken ondersteuningsverklaringen",
    );
    await expect(omissionsPage.textfieldLetter).toHaveValue("Eigen tekst");
  });
});
