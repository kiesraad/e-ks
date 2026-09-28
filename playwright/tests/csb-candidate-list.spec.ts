import { expect } from "@playwright/test";
import { test } from "./fixtures.ts";
import { CsbCandidateListPage } from "./pages/csb/csbCandidateListPage.ts";
import { CsbOmissionsListPage } from "./pages/csb/csbOmissionsListPage.ts";
import { CsbPoliticalGroupPage } from "./pages/csb/csbPoliticalGroupPage.ts";

test.describe("check candidate list and add corrections and omissions", async () => {
  test("for single list", async ({ csbImport }) => {
    const { page, groupName } = csbImport;
    const politicalGroupPage = new CsbPoliticalGroupPage(page);
    const omissionsPage = new CsbOmissionsListPage(page);
    const candidateListPage = new CsbCandidateListPage(page);

    await politicalGroupPage.selectedGroup(groupName);
    await politicalGroupPage.linkCandidateList.first().click();

    await expect(candidateListPage.headerCandidateList).toBeVisible();

    // Get the electoral district from the candidate list page
    const districts = ["Drenthe", "Groningen", "Overijssel"];
    const selectedDistrict = await candidateListPage.getElectoralDistrict(
      page,
      districts,
    );

    // Add each type of omission, verify and then remove
    const omissions = [
      {
        button: omissionsPage.buttonAuthoriseAppelation,
        text: "De machtiging aanduiding ontbreekt",
        resolvable: true,
      },
      {
        button: omissionsPage.buttonAuthorisedAgent,
        text: "De gemachtigde is niet geregistreerd",
        resolvable: true,
      },
    ];

    for (const { button, text, resolvable } of omissions) {
      await candidateListPage.linkAddOmission.click();
      await page.waitForURL(/\/omission\//);
      await expect(
        page.getByRole("heading", { name: "Verzuimen - Kandidatenlijst" }),
      ).toBeVisible();
      await omissionsPage.expectOnlySelectedDistrictChecked(
        page,
        districts,
        selectedDistrict,
      );
      await button.click();
      if (resolvable) {
        await expect(omissionsPage.checkboxRecoverable).toBeChecked();
        await omissionsPage.textfieldLetter.fill("Testtoevoeging");
      }
      await omissionsPage.buttonAddAndClose.click();
      await expect(page.locator("form.overlay")).toBeHidden();
      await candidateListPage.linkManageOmissions.click();
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
      await omissionsPage.expectOnlySelectedDistrictAdded(
        page,
        districts,
        selectedDistrict,
      );
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
    const omissionsPage = new CsbOmissionsListPage(page);
    const candidateListPage = new CsbCandidateListPage(page);

    await politicalGroupPage.selectedGroup(groupName);
    await politicalGroupPage.linkCandidateList.first().click();

    await expect(candidateListPage.headerCandidateList).toBeVisible();

    // Get the electoral district from the candidate list page
    const districts = ["Drenthe", "Groningen", "Overijssel"];
    const selectedDistrict = await candidateListPage.getElectoralDistrict(
      page,
      districts,
    );

    // Add each type of omission, verify and then remove
    const omissions = [
      {
        button: omissionsPage.buttonAuthoriseCombination,
        text: "De machtiging samenvoeging ontbreekt",
        resolvable: true,
      },
      {
        button: omissionsPage.buttonAuthorisedAgentCombination,
        text: "De gemachtigde(n) is/zijn niet geregistreerd",
        resolvable: true,
      },
    ];

    for (const { button, text, resolvable } of omissions) {
      await candidateListPage.linkAddOmission.click();
      await page.waitForURL(/\/omission\//);
      await expect(
        page.getByRole("heading", { name: "Verzuimen - Kandidatenlijst" }),
      ).toBeVisible();
      await omissionsPage.expectOnlySelectedDistrictChecked(
        page,
        districts,
        selectedDistrict,
      );
      await omissionsPage.checkboxAllLists.check();
      await button.click();
      if (resolvable) {
        await expect(omissionsPage.checkboxRecoverable).toBeChecked();
        await omissionsPage.textfieldLetter.fill("Testtoevoeging");
      }
      await omissionsPage.buttonAddAndClose.click();
      await expect(page.locator("form.overlay")).toBeHidden();
      await candidateListPage.linkManageOmissions.click();
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
      await omissionsPage.expectAllDistrictsAdded(page, districts);
      await omissionsPage.clickRemoveOmission();
      await expect(
        page.getByText("Er zijn nog geen verzuimen toegevoegd."),
      ).toBeVisible();
      await omissionsPage.linkClose.click();
    }
  });
});
