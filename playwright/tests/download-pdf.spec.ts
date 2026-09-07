// Use cases:
// De applicatie genereert de benodigde documenten.
// De lijstinleveraar downloadt de benodigde documenten in pdf.
// Er zijn na het printen toch fouten, de lijstinleveraar geeft in de applicatie aan dat er fouten zijn, de lijstinleveraar bewerkt de gegevens

import { stat } from "node:fs/promises";
import { expect, type Page } from "@playwright/test";
import { test } from "./fixtures.ts";
import { CandidateListsOverviewPage } from "./pages/pg/candidateListsOverviewPage.ts";
import { EditListDetailsPage } from "./pages/pg/editListDetailsPage.ts";
import { FinalisePage } from "./pages/pg/finalisePage.ts";
import { ManageCandidateListPage } from "./pages/pg/manageCandidateListPage.ts";

test.describe("download documents", async () => {
  const existingCandidates = ["Nagelhout", "Braber"];

  async function setupCandidateList(page: Page, district: string) {
    await page.goto("/candidate-lists");
    await new CandidateListsOverviewPage(page).buttonAddList.click();
    await new EditListDetailsPage(page).addDistricts([district]);
    await new ManageCandidateListPage(page).addExistingCandidates(
      existingCandidates,
    );
  }

  test("download export", async ({ deleteExistingCandidateLists: page }) => {
    await setupCandidateList(page, "Gelderland");
    await page.goto("/finalise");

    const downloadPromise = page.waitForEvent("download");
    await new FinalisePage(page).linkDownloadNl.click();
    const download = await downloadPromise;

    expect(download.suggestedFilename()).toMatch(/^[a-z0-9-]+-v\d+\.zip$/);
    expect((await stat(await download.path())).size).toBeGreaterThan(1024);

    await expect(new FinalisePage(page).linkDownloadFry).not.toBeVisible();
  });

  test("download frisian export", async ({
    provincialCouncilFrisianElection: page,
  }) => {
    await page.goto("/finalise");

    const downloadPromise = page.waitForEvent("download");
    await new FinalisePage(page).linkDownloadNl.click();
    const download = await downloadPromise;

    expect(download.suggestedFilename()).toMatch(/^[a-z0-9-]+-v\d+\.zip$/);
    expect((await stat(await download.path())).size).toBeGreaterThan(1024);

    const downloadPromise2 = page.waitForEvent("download");
    await new FinalisePage(page).linkDownloadFry.click();
    const download2 = await downloadPromise2;

    expect(download2.suggestedFilename()).toMatch(/^[a-z0-9-]+-v\d+-fry\.zip$/);
    expect((await stat(await download2.path())).size).toBeGreaterThan(1024);
  });

  test("download and edit", async ({ deleteExistingCandidateLists: page }) => {
    await setupCandidateList(page, "Gelderland");
    await page.goto("/finalise");

    const downloadPromise = page.waitForEvent("download");
    const finalisePage = new FinalisePage(page);
    await finalisePage.linkDownloadNl.click();
    const download = await downloadPromise;

    expect(download.suggestedFilename()).toMatch(/^[a-z0-9-]+-v\d+\.zip$/);
    expect((await stat(await download.path())).size).toBeGreaterThan(1024);

    await finalisePage.linkCandidateList.click();
    await expect(
      page.getByText("Let op: de documenten zijn al gedownload."),
    ).toBeVisible();
  });
});
