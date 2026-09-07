import { expect } from "@playwright/test";
import { test } from "./fixtures.ts";
import { CsbCandidateListPage } from "./pages/csb/csbCandidateListPage.ts";
import { CsbCandidatePage } from "./pages/csb/csbCandidatePage.ts";
import { CsbCorrectionsPage } from "./pages/csb/csbCorrectionsPage.ts";
import { CsbOmissionsCandidatePage } from "./pages/csb/csbOmissionsCandidatePage.ts";
import { CsbPoliticalGroupPage } from "./pages/csb/csbPoliticalGroupPage.ts";

test.describe("check candidate and add corrections and omissions", async () => {
  test("add corrections", async ({ csbImport }) => {
    const { page, groupName } = csbImport;
    const politicalGroupPage = new CsbPoliticalGroupPage(page);
    const candidateListPage = new CsbCandidateListPage(page);
    const correctionsPage = new CsbCorrectionsPage(page);
    const candidatePage = new CsbCandidatePage(page);

    await politicalGroupPage.selectedGroup(groupName);
    await politicalGroupPage.linkCandidateList.first().click();

    await expect(candidateListPage.headerCandidateList).toBeVisible();
    await page
      .getByRole("cell", { name: "Peereboom, P. (Patricia) (v)" })
      .click();
    await expect(
      page.getByRole("heading", { level: 1, name: "Peereboom, P. (Patricia)" }),
    ).toBeVisible();
    await candidatePage.linkInitials.click();
    await correctionsPage.addCorrection("Z.");
    await expect(candidatePage.textCorrectedInitials).toHaveText("Z.");
    await candidatePage.linkLastNamePrefix.click();
    await correctionsPage.addCorrection("van der");
    await expect(candidatePage.textCorrectedLastNamePrefix).toHaveText(
      "van der",
    );
    await candidatePage.linkLastName.click();
    await correctionsPage.addCorrection("Pereboom");
    await expect(candidatePage.textCorrectedLastName).toHaveText("Pereboom");
    await candidatePage.linkDateOfBirth.click();
    await correctionsPage.addCorrection("12");
    await expect(candidatePage.textCorrectedDateOfBirth).toHaveText(
      "12-02-1983",
    );
    await candidatePage.linkPlaceOfResidence.first().click();
    await correctionsPage.addCorrection("Amsterdam");
    await expect(
      candidatePage.textCorrectedPlaceOfResidence.first(),
    ).toHaveText("Amsterdam");
  });

  test("for single list", async ({ csbImport }) => {
    const { page, groupName } = csbImport;
    const politicalGroupPage = new CsbPoliticalGroupPage(page);
    const candidateListPage = new CsbCandidateListPage(page);
    const candidatePage = new CsbCandidatePage(page);
    const omissionsPage = new CsbOmissionsCandidatePage(page);

    await politicalGroupPage.selectedGroup(groupName);
    await politicalGroupPage.linkCandidateList.first().click();

    await expect(candidateListPage.headerCandidateList).toBeVisible();
    await page
      .getByRole("cell", { name: "Peereboom, P. (Patricia) (v)" })
      .click();
    await expect(
      page.getByRole("heading", { level: 1, name: "Peereboom, P. (Patricia)" }),
    ).toBeVisible();

    // Get the electoral district from the candidate list page
    const districts = ["Drenthe", "Groningen", "Overijssel"];
    const selectedDistrict = await candidatePage.getElectoralDistrict(
      page,
      districts,
    );

    // Add each type of omission, verify and then remove
    const omissions = [
      {
        button: omissionsPage.buttonWrongGender,
        text: "Bij kandidaat is het onjuiste geslacht (x) vermeld",
        resolvable: true,
      },

      {
        button: omissionsPage.buttonAuthorisedPerson,
        text: "Gemachtigde kandidaat ontbreekt",
        resolvable: true,
      },
    ];

    for (const { button, text, resolvable } of omissions) {
      await candidatePage.linkAddOmission.click();
      await page.waitForURL(/\/omission\//);
      await expect(
        page.getByRole("heading", {
          name: "Verzuimen: Peereboom, P. (Patricia)",
        }),
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
      await candidatePage.linkManageOmissions.click();
      await page.waitForURL(/\/omission\//);
      await expect(page.getByText(text).first()).toBeVisible();
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
    const candidateListPage = new CsbCandidateListPage(page);
    const candidatePage = new CsbCandidatePage(page);
    const omissionsPage = new CsbOmissionsCandidatePage(page);

    await politicalGroupPage.selectedGroup(groupName);
    await politicalGroupPage.linkCandidateList.first().click();

    await expect(candidateListPage.headerCandidateList).toBeVisible();
    await page
      .getByRole("cell", { name: "Peereboom, P. (Patricia) (v)" })
      .click();
    await expect(
      page.getByRole("heading", { level: 1, name: "Peereboom, P. (Patricia)" }),
    ).toBeVisible();

    // Get the electoral district from the candidate list page
    const districts = ["Drenthe", "Groningen", "Overijssel"];
    const selectedDistrict = await candidatePage.getElectoralDistrict(
      page,
      districts,
    );

    // Add each type of omission, verify and then remove
    const omissions = [
      {
        button: omissionsPage.buttonDifferenceH1,
        text: "De kandidatenlijst op Model H 1 en op de instemmingsverklaring komen niet overeen",
        resolvable: true,
      },
      {
        button: omissionsPage.buttonMissingCopyID,
        text: "Kopie ID ontbreekt",
        resolvable: true,
      },
    ];

    for (const { button, text, resolvable } of omissions) {
      await candidatePage.linkAddOmission.click();
      await page.waitForURL(/\/omission\//);
      await expect(
        page.getByRole("heading", {
          name: "Verzuimen: Peereboom, P. (Patricia)",
        }),
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
      await candidatePage.linkManageOmissions.click();
      await page.waitForURL(/\/omission\//);
      await expect(page.getByText(text).first()).toBeVisible();
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
