import { stat } from "node:fs/promises";
import { expect } from "@playwright/test";
import { test } from "./fixtures.ts";
import type { Candidate } from "./models/candidate.ts";
import type { ListSubmitter } from "./models/listSubmitter.ts";
import type { NameAuthorisation } from "./models/nameAuthorisation.ts";
import { CandidateListsOverviewPage } from "./pages/pg/candidateListsOverviewPage.ts";
import { CsvImportExportPage } from "./pages/pg/csvImportExportPage.ts";
import { FinalisePage } from "./pages/pg/finalisePage.ts";
import { ListDesignationPage } from "./pages/pg/listDesignationPage.ts";
import { ListSubmittersPage } from "./pages/pg/listSubmittersPage.ts";
import { ManageCandidateListPage } from "./pages/pg/manageCandidateListPage.ts";
import { NameAuthorisationPage } from "./pages/pg/nameAuthorisationPage.ts";
import { OverviewPage } from "./pages/pg/overviewPage.ts";
import { PoliticalGroupPage } from "./pages/pg/politicalGroupPage.ts";
import { SubstituteSubmittersPage } from "./pages/pg/substituteSubmittersPage.ts";

test.describe("End-to-end", () => {
  // These walk the entire application flow (~20+ navigations, a CSV upload and
  // a download) so they need more than the default per-test timeout.
  test.beforeEach(() => {
    test.slow();
  });

  test("happy flow", async ({ noExistingData: page }) => {
    //navigate from home page
    const overviewPage = new OverviewPage(page);
    await overviewPage.linkGeneralInformation.click();
    await page.waitForURL("/political-group*");

    //fill in general information
    const listDesignationPage = new ListDesignationPage(page);
    await listDesignationPage.selectStandalone.check();
    await listDesignationPage.buttonSaveAndNext.click();
    await page.waitForURL("/political-group/information*");

    const politicalGroupPage = new PoliticalGroupPage(page);
    await politicalGroupPage.selectNoSeats.check();
    await politicalGroupPage.textfieldRegisteredDesignation.fill("Test Partij");
    await politicalGroupPage.buttonSaveAndNext.click();
    await page.waitForURL("/political-group/name-authorisation*");

    const authorisation: NameAuthorisation = {
      initials: "T",
      lastNamePrefix: "van",
      lastName: "Tester",
      legalName: "Kiesraad Test Partij",
    };
    const nameAuthorisationPage = new NameAuthorisationPage(page);
    await nameAuthorisationPage.addNameAuthorisation(authorisation);
    await nameAuthorisationPage.buttonNext.click();
    await page.waitForURL("/political-group/list-submitter*");

    const submitter: ListSubmitter = {
      initials: "L",
      lastNamePrefix: "de",
      lastName: "Inleveraar",
      postalCode: "1234 AB",
      houseNumber: "1",
      houseNumberAddition: "a",
      streetName: "Teststraat",
      locality: "Teststad",
    };
    const listSubmittersPage = new ListSubmittersPage(page);
    await listSubmittersPage.setListSubmitter(submitter);

    const substitute: ListSubmitter = {
      initials: "V",
      lastNamePrefix: "ter",
      lastName: "Vervanger",
      postalCode: "5678 CD",
      houseNumber: "2",
      houseNumberAddition: "b",
      streetName: "Testlaan",
      locality: "Testdorp",
    };
    const substituteSubmittersPage = new SubstituteSubmittersPage(page);
    await substituteSubmittersPage.addSubstituteSubmitter(substitute);
    await listSubmittersPage.buttonNext.click();
    await page.waitForURL("/");

    //create candidate list and add candidates
    await overviewPage.linkCandidateList.click();
    await page.waitForURL("/candidate-lists");
    const candidateListsOverviewPage = new CandidateListsOverviewPage(page);
    await candidateListsOverviewPage.buttonAddList.click();
    const manageCandidateListPage = new ManageCandidateListPage(page);
    await manageCandidateListPage.selectDistricts([
      "Selecteer alle kieskringen",
    ]);
    await expect(manageCandidateListPage.buttonEditList).toBeVisible();

    await manageCandidateListPage.buttonCSV.click();
    const csvImportExport = new CsvImportExportPage(page);
    await csvImportExport.uploadCsvFile("candidate-list-export-nh-1.csv");
    await expect(manageCandidateListPage.headingCandidateList).toBeVisible();
    await expect(
      await manageCandidateListPage.getCandidateLocator("Groot, de"),
    ).toBeVisible();

    const candidate: Candidate = {
      initials: "K",
      lastName: "Kandidaat",
      firstName: "Kees",
      locality: "Rotterdam",
      bsn: "000000024",
      gender: "man",
      dateOfBirth: {
        day: "31",
        month: "01",
        year: "1980",
      },
      postalCode: "1234 AB",
      houseNumber: "1",
      houseNumberAddition: "a",
      streetName: "Kandidatenstraat",
      countryCode: "NL",
    };

    await manageCandidateListPage.addNewCandidates([candidate]);
    for (const newCandidate of [candidate]) {
      await expect(
        await manageCandidateListPage.getCandidateLocator(
          newCandidate.lastName,
        ),
      ).toBeVisible();
    }

    //finalise list
    await manageCandidateListPage.buttonOverviewPage.click();
    await page.waitForURL("/");
    await overviewPage.linkFinalise.click();
    await page.waitForURL("/finalise");
    const _submitPage = new FinalisePage(page);
    const downloadPromise = page.waitForEvent("download");
    await new FinalisePage(page).linkDownloadNl.click();
    const download = await downloadPromise;

    expect(download.suggestedFilename()).toMatch(/^[a-z0-9-]+-v\d+\.zip$/);
    expect((await stat(await download.path())).size).toBeGreaterThan(1024);
  });

  test("with errors", async ({ noExistingData: page }) => {
    //navigate from home page
    const overviewPage = new OverviewPage(page);
    await overviewPage.linkGeneralInformation.click();
    await page.waitForURL("/political-group*");

    //fill in general information
    const listDesignationPage = new ListDesignationPage(page);
    await listDesignationPage.selectStandalone.check();
    await listDesignationPage.buttonSaveAndNext.click();
    await page.waitForURL("/political-group/information*");

    const politicalGroupPage = new PoliticalGroupPage(page);
    await politicalGroupPage.selectNoSeats.check();
    await politicalGroupPage.buttonSaveAndNext.click();
    await page.waitForURL("/political-group/name-authorisation*");

    const authorisation: NameAuthorisation = {
      initials: "T",
      lastNamePrefix: "van",
      lastName: "Tester",
      legalName: "Kiesraad Test Partij",
    };
    const nameAuthorisationPage = new NameAuthorisationPage(page);
    await nameAuthorisationPage.addNameAuthorisation(authorisation);
    await nameAuthorisationPage.buttonNext.click();
    await page.waitForURL("/political-group/list-submitter*");

    const submitter: ListSubmitter = {
      initials: "L",
      lastNamePrefix: "de",
      lastName: "Inleveraar",
      postalCode: "1234 AB",
      houseNumber: "1",
      houseNumberAddition: "a",
      streetName: "Teststraat",
      locality: "Teststad",
    };
    const listSubmittersPage = new ListSubmittersPage(page);
    await listSubmittersPage.setListSubmitter(submitter);

    const substitute: ListSubmitter = {
      initials: "V",
      lastNamePrefix: "ter",
      lastName: "Vervanger",
      postalCode: "5678 CD",
      houseNumber: "2",
      houseNumberAddition: "b",
      streetName: "Testlaan",
      locality: "Testdorp",
    };
    const substituteSubmittersPage = new SubstituteSubmittersPage(page);
    await substituteSubmittersPage.addSubstituteSubmitter(substitute);
    await listSubmittersPage.buttonNext.click();
    await page.waitForURL("/");

    //create candidate list and add candidates
    await overviewPage.linkCandidateList.click();
    await page.waitForURL("/candidate-lists");
    const candidateListsOverviewPage = new CandidateListsOverviewPage(page);
    await candidateListsOverviewPage.buttonAddList.click();
    const manageCandidateListPage = new ManageCandidateListPage(page);
    await manageCandidateListPage.selectDistricts([
      "Selecteer alle kieskringen",
    ]);
    await expect(manageCandidateListPage.buttonEditList).toBeVisible();

    await manageCandidateListPage.buttonCSV.click();
    const csvImportExport = new CsvImportExportPage(page);
    await csvImportExport.uploadCsvFile("candidate-list-export-nh.csv");
    await expect(csvImportExport.textFailure).toBeVisible();
    await csvImportExport.buttonClose.click();
    await expect(manageCandidateListPage.headingCandidateList).toBeVisible();
    await expect(
      await manageCandidateListPage.getCandidateLocator("Groot, de"),
    ).not.toBeVisible();

    const candidate: Candidate = {
      initials: "K",
      lastName: "Kandidaat",
      firstName: "Kees",
      locality: "Rotterdam",
      bsn: "000000024",
      gender: "man",
      dateOfBirth: {
        day: "31",
        month: "01",
        year: "1980",
      },
      postalCode: "1234 AB",
      houseNumber: "1",
      houseNumberAddition: "a",
      streetName: "Kandidatenstraat",
      countryCode: "NL",
    };

    await manageCandidateListPage.addNewCandidates([candidate]);
    for (const newCandidate of [candidate]) {
      await expect(
        await manageCandidateListPage.getCandidateLocator(
          newCandidate.lastName,
        ),
      ).toBeVisible();
    }

    //finalise list
    await manageCandidateListPage.buttonOverviewPage.click();
    await page.waitForURL("/");
    await overviewPage.linkFinalise.click();
    await page.waitForURL("/finalise");
    const downloadLink = page.locator("a", { hasText: "Alles in één zip" });
    await expect(downloadLink).toBeVisible();
    await expect(downloadLink).toHaveAttribute("aria-disabled", "true");
    await expect(downloadLink).toHaveClass(/disabled/);
  });
});
