// Use cases:
// e-KS constateert dat er BRP fouten zijn, CSB bevestigt de BRP fouten
// e-KS constateert dat er BRP fouten zijn, CSB corrigeert de BRP ambtshalve in het systeem
// e-KS constateert dat er BRP fouten zijn, CSB stelt vast dat er verzuimen zijn

import { expect } from "@playwright/test";
import { test } from "./fixtures.ts";
import type { Candidate } from "./models/candidate.ts";
import { CsbExaminationPage } from "./pages/csb/csbExaminationPage.ts";
import { CsbImportPage } from "./pages/csb/csbImportPage.ts";
import { CsbPoliticalGroupPage } from "./pages/csb/csbPoliticalGroupPage.ts";
import { CandidateListsOverviewPage } from "./pages/pg/candidateListsOverviewPage.ts";
import { EditListDetailsPage } from "./pages/pg/editListDetailsPage.ts";
import { ManageCandidateListPage } from "./pages/pg/manageCandidateListPage.ts";

test.describe("check for BRP errors", async () => {
  test("happy flow: no errors found", async ({ csbImport }) => {
    const { page } = csbImport;
    const groupPage = new CsbPoliticalGroupPage(page);
    const examinationPage = new CsbExaminationPage(page);
    const importPage = new CsbImportPage(page);

    const candidate: Candidate = {
      initials: "H",
      lastName: "Nagelhout",
      firstName: "Hubertus",
      gender: "male",
      dateOfBirth: { day: "06", month: "06", year: "1975" },
      bsn: "999995972",
      postalCode: "6718ZD",
      houseNumber: "9",
      streetName: "Knuttelweg",
      locality: "Ede",
    };

    // create new group with one correct candidate
    await page.goto("/csb/examination");
    await examinationPage.linkAddPoliticalGroup.click();
    await importPage.buttonAddNew.click();
    await page.goto("/candidate-lists");
    await new CandidateListsOverviewPage(page).buttonAddList.click();
    await new EditListDetailsPage(page).addDistricts(["Overijssel"]);
    await new ManageCandidateListPage(page).addNewCandidates([candidate]);
    await page.getByRole("button", { name: "Terug naar onderzoek" }).click();

    // do the BRP check
    await expect(
      page.getByText("De kandidaten zijn nog niet tegen de BRP gecontroleerd."),
    ).toBeVisible();
    await groupPage.startBrpCheck();
    await expect(page.getByText("Geen BRP fouten")).toBeVisible();

    // delete group to clean up
    await groupPage.linkDelete.click();
    await groupPage.buttonDeleteConfirm.click();
    await page.waitForURL(/\/csb\/examination$/);
  });

  test("flow with BRP errors", async ({ csbImport }) => {
    // paced BRP check plus ~30 navigations for the 9 errors
    test.slow();
    const { page } = csbImport;
    const groupPage = new CsbPoliticalGroupPage(page);
    const examinationPage = new CsbExaminationPage(page);
    const importPage = new CsbImportPage(page);

    // incorrect initials, last name, gender, date of birth, locality
    const candidateOne: Candidate = {
      initials: "H.J.",
      lastName: "Nagelhut",
      firstName: "Hubertus",
      gender: "female",
      dateOfBirth: { day: "06", month: "07", year: "1975" },
      bsn: "999995972",
      postalCode: "6718ZD",
      houseNumber: "9",
      streetName: "Knuttelweg",
      locality: "Lutjebroek",
    };

    // incorrect bsn, person excluded from electoral rights and no known place of residence
    const candidateTwo: Candidate = {
      initials: "Y.J.",
      lastName: "Kierkegaard",
      firstName: "Yvonne",
      gender: "female",
      dateOfBirth: { day: "15", month: "11", year: "1956" },
      bsn: "000000012",
      postalCode: " 3037XB",
      houseNumber: "15",
      streetName: " Rodenrijselaan",
      locality: "Rotterdam",
    };

    const expectedErrors = [
      "De achternaam mag volgens de BRP zijn: Nagelhout, Speelman, Nagelhout-Speelman, Speelman-Nagelhout",
      "Voorletters wijkt af van de BRP, waarin staat: H",
      "Geslacht wijkt af van de BRP, waarin staat: man",
      "Geboortedatum wijkt af van de BRP, waarin staat: 06-06-1975",
      "Woonplaats wijkt af van de BRP, waarin staat: Ede",
      "Er is geen persoon in de BRP met dit burgerservicenummer",
      "Op de overige persoonsgegevens is precies één persoon in de BRP gevonden, met burgerservicenummer 999991929. De gegevens hieronder zijn met die persoon vergeleken.",
      "De BRP bevat geen Woonplaats voor deze kandidaat",
      "Volgens de BRP is deze kandidaat uitgesloten van het kiesrecht",
    ];

    // create new group with incorrect candidates
    await page.goto("/csb/examination");
    await examinationPage.linkAddPoliticalGroup.click();
    await importPage.buttonAddNew.click();
    await page.goto("/candidate-lists");
    await new CandidateListsOverviewPage(page).buttonAddList.click();
    await new EditListDetailsPage(page).addDistricts(["Overijssel"]);
    await new ManageCandidateListPage(page).addNewCandidates([
      candidateOne,
      candidateTwo,
    ]);
    await page.getByRole("button", { name: "Terug naar onderzoek" }).click();

    // do the BRP check
    await expect(
      page.getByText("De kandidaten zijn nog niet tegen de BRP gecontroleerd."),
    ).toBeVisible();
    await groupPage.startBrpCheck();
    await expect(page.getByText("9 BRP fouten")).toBeVisible();
    await groupPage.linkAllErrors.click();

    // check that all expected errors are present and mark as handled
    for (const error of expectedErrors) {
      await expect(page.getByText(error)).toBeVisible();
      await expect(page.getByText(error)).toHaveCSS(
        "color",
        "rgb(176, 45, 69)",
      );
      await page.getByRole("link", { name: error }).click();
      await page
        .getByRole("row")
        .filter({ hasText: error })
        .getByRole("button", { name: "Afgehandeld", exact: true })
        .click();
      await page.getByRole("link", { name: "Naar alle fouten" }).click();
      await expect(page.getByText(error)).toHaveCSS(
        "color",
        "rgb(102, 112, 133)",
      );
    }

    // delete group to clean up
    await groupPage.buttonBack.click();
    await groupPage.linkDelete.click();
    await groupPage.buttonDeleteConfirm.click();
    await page.waitForURL(/\/csb\/examination$/);
  });
});
