// Use cases:
// De applicatie valideert dat alle benodigde gegevens correct zijn ingevuld.
// Er staan kandidaten op die niet op de lijst horen, de lijstinleveraar verwijdert kandidaten
// gegevens kloppen niet (kandidatenlijsten), de lijstinleveraar corrigeert de gegevens (kandidatenlijsten)
// gegevens kloppen niet (basisgegevens), de lijstinleveraar corrigeert de gegevens (basisgegevens)
// Gegevens zijn niet volledig (basisgegevens), lijstinleveraar vult de gegevens aan
// Na validatie: gegevens kloppen niet, applicatie geeft (blocking) waarschuwing, lijstinleveraar corrigeert de gegevens
// Na validatie: gegevens zijn niet volledig, applicatie geeft (blocking) waarschuwing, lijstinleveraar vult de gegevens aan

import { expect } from "@playwright/test";
import { test } from "./fixtures.ts";
import type { NameAuthorisation } from "./models/nameAuthorisation.ts";
import { CandidateListsOverviewPage } from "./pages/pg/candidateListsOverviewPage.ts";
import { CreatePersonPage } from "./pages/pg/createPersonPage.ts";
import { CsvImportExportPage } from "./pages/pg/csvImportExportPage.ts";
import { EditListDetailsPage } from "./pages/pg/editListDetailsPage.ts";
import { FinalisePage } from "./pages/pg/finalisePage.ts";
import { ListSubmittersPage } from "./pages/pg/listSubmittersPage.ts";
import { ManageCandidateListPage } from "./pages/pg/manageCandidateListPage.ts";
import { NameAuthorisationPage } from "./pages/pg/nameAuthorisationPage.ts";
import { OverviewPage } from "./pages/pg/overviewPage.ts";
import { PoliticalGroupPage } from "./pages/pg/politicalGroupPage.ts";

// A save redirects to `/finalise?&success=true&overlay=true`; the page scripts
// strip those params again, so waiting on the bare path waits on that cleanup
// rather than on the redirect. Hence the trailing `*`.
test.describe("fix submit warnings", async () => {
  test("general information", async ({ noExistingData: page }) => {
    const finalisePage = new FinalisePage(page);
    const politicalGroupPage = new PoliticalGroupPage(page);
    const listSubmittersPage = new ListSubmittersPage(page);
    const nameAuthorisationPage = new NameAuthorisationPage(page);
    const overviewPage = new OverviewPage(page);
    const authorisation: NameAuthorisation = {
      initials: "K",
      lastName: "Jansen",
      legalName: "Kiesraad Demo Partij",
    };

    await page.goto("/finalise");
    await finalisePage.linkRegisteredDesignation.click();
    await expect(politicalGroupPage.headerGeneralInformation).toBeVisible();
    await politicalGroupPage.setRegisteredDesignation("Test");
    await politicalGroupPage.save();
    await nameAuthorisationPage.buttonNext.click();
    await listSubmittersPage.buttonNext.click();
    await overviewPage.linkFinalise.click();
    await page.waitForURL("/finalise*");
    await expect(finalisePage.linkRegisteredDesignation).not.toBeVisible();

    await finalisePage.linkNoLegalName.click();
    await nameAuthorisationPage.editNameAuthorisation([authorisation]);
    await page.waitForURL("/finalise*");
    await expect(finalisePage.linkNoLegalName).not.toBeVisible();
  });

  test("candidates", async ({ login: page }) => {
    const finalisePage = new FinalisePage(page);
    const createPersonPage = new CreatePersonPage(page);

    await page.goto("/finalise");
    await finalisePage.linkBSN.click();
    await createPersonPage.checkboxNoBSN.check();
    await createPersonPage.buttonNext.click();
    await page.waitForURL("/finalise*");
    await expect(finalisePage.linkBSN).not.toBeVisible();

    await finalisePage.linkIncorrectDate.first().click();
    await createPersonPage.textfieldYearOfBirth.fill("1925");
    await createPersonPage.buttonNext.click();
    await page.waitForURL("/finalise*");

    await finalisePage.linkIncorrectDate.click();
    await createPersonPage.textfieldYearOfBirth.fill("1990");
    await createPersonPage.buttonNext.click();
    await page.waitForURL("/finalise*");
    await expect(finalisePage.linkIncorrectDate).not.toBeVisible();
  });

  test("candidate lists", async ({ login: page }) => {
    const overviewPage = new OverviewPage(page);
    const finalisePage = new FinalisePage(page);
    const manageCandidateListPage = new ManageCandidateListPage(page);

    await page.goto("/finalise");
    await finalisePage.linkTooManyCandidates.first().click();
    await manageCandidateListPage.deleteCandidates([
      "Nagelhout",
      "Meerman",
      "Altena",
      "Smit",
      "Bruin",
    ]);
    await manageCandidateListPage.buttonOverviewPage.click();
    await page.waitForURL("/");
    await overviewPage.linkFinalise.click();
    await page.waitForURL("/finalise*");
    await expect(finalisePage.linkTooManyCandidates).not.toBeVisible();
  });

  test("csv import", async ({ deleteExistingCandidateLists: page }) => {
    const overviewPage = new OverviewPage(page);
    await page.goto("/candidate-lists");
    await new CandidateListsOverviewPage(page).buttonAddList.click();
    await new EditListDetailsPage(page).addDistricts(["Saba"]);
    await new ManageCandidateListPage(page).buttonCSV.click();
    const csvImportExport = new CsvImportExportPage(page);
    await csvImportExport.uploadCsvFile("candidate-list-warnings.csv");
    const manageCandidateListPage = new ManageCandidateListPage(page);
    await expect(manageCandidateListPage.headingCandidateList).toBeVisible();
    await expect(
      await manageCandidateListPage.getCandidateLocator("Smit"),
    ).toBeVisible();
    await manageCandidateListPage.buttonOverviewPage.click();
    await page.waitForURL("/");
    await overviewPage.linkFinalise.click();
    await page.waitForURL("/finalise*");
    const finalisePage = new FinalisePage(page);
    await expect(finalisePage.linkBSN).toBeVisible();
    await expect(finalisePage.linkDateOfBirth).toBeVisible();
    await expect(finalisePage.linkPlaceOfResidenceNotFound).toBeVisible();
    await expect(finalisePage.linkAdressIncomplete).toBeVisible();
    await expect(finalisePage.linkAdressNotFound.first()).toBeVisible();
    await expect(finalisePage.linkTooYoung.first()).toBeVisible();
  });
});
