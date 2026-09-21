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
        button: omissionsPage.buttonMissingOneDistrict,
        text: "Voor één kieskring ontbreken ondersteuningsverklaringen",
        resolvable: true,
      },
      {
        button: omissionsPage.buttonDeclarationsNotValid,
        text: "Geen geldige ondersteuningsverklaringen ingeleverd",
        resolvable: true,
      },
    ];

    for (const { button, text, resolvable } of omissions) {
      await politicalGroupPage.linkSupportDeclarations.click();
      await page.waitForURL(/\/omission\//);
      await expect(omissionsPage.headerDeclarationsOfSupport).toBeVisible();

      await page.getByRole("checkbox", { name: "1. Groningen" }).check();
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
        button: omissionsPage.buttonMissingMultipleDistricts,
        text: "Voor meerdere kieskringen ontbreken ondersteuningsverklaringen",
        resolvable: true,
      },
      {
        button: omissionsPage.buttonDeclarationsNotValid,
        text: "Geen geldige ondersteuningsverklaringen ingeleverd",
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
        button: omissionsPage.buttonMissingAllDistricts,
        text: "Voor alle kieskringen ontbreken ondersteuningsverklaringen",
        resolvable: true,
      },
      {
        button: omissionsPage.buttonDeclarationsNotValid,
        text: "Geen geldige ondersteuningsverklaringen ingeleverd",
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
});
