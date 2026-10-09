import { expect } from "@playwright/test";
import { test } from "./fixtures.ts";
import { CsbImportPage } from "./pages/csb/csbImportPage.ts";
import { CsbOverviewPage } from "./pages/csb/csbOverviewPage.ts";
import { CsbPreSubmissionPage } from "./pages/csb/csbPreSubmissionPage.ts";

test("import a political group for the pre-submission check by hash", async ({
  csbLogin,
}) => {
  const { page, groupName, lastEventHash } = csbLogin;
  expect(lastEventHash).not.toBe("");

  const overviewPage = new CsbOverviewPage(page);
  const preSubmissionPage = new CsbPreSubmissionPage(page);
  const importPage = new CsbImportPage(page);

  await expect(overviewPage.headerElection).toBeVisible();
  await overviewPage.linkPreSubmission.click();
  await expect(preSubmissionPage.header).toBeVisible();

  await preSubmissionPage.linkAddPoliticalGroup.click();
  await expect(importPage.headerImport).toBeVisible();
  await importPage.textfieldHashcode.fill(lastEventHash);
  await Promise.all([
    page.waitForURL(/\/csb\/pre-submission\/[^/]+$/),
    importPage.buttonImport.click(),
  ]);

  // The group page lists the BRP errors and nothing else.
  await expect(page.getByRole("heading", { name: groupName })).toBeVisible();
  await expect(preSubmissionPage.headerErrors).toBeVisible();
  await expect(
    page.getByRole("link", { name: "Kandidatenlijst controleren" }),
  ).toHaveCount(0);

  // The group is listed for the pre-submission check...
  await preSubmissionPage.buttonBack.click();
  await expect(preSubmissionPage.header).toBeVisible();
  await expect(preSubmissionPage.row(groupName)).toBeVisible();

  // ...and not for the examination.
  await page.goto("/csb/examination");
  await expect(
    page.getByRole("cell", { name: groupName, exact: true }),
  ).toHaveCount(0);
});

test("importing the same political group again for the pre-submission check warns first", async ({
  csbLogin,
}) => {
  const { page, groupName, lastEventHash } = csbLogin;
  const overviewPage = new CsbOverviewPage(page);
  const preSubmissionPage = new CsbPreSubmissionPage(page);
  const importPage = new CsbImportPage(page);

  await overviewPage.linkPreSubmission.click();
  await preSubmissionPage.linkAddPoliticalGroup.click();
  await importPage.textfieldHashcode.fill(lastEventHash);
  await Promise.all([
    page.waitForURL(/\/csb\/pre-submission\/[^/]+$/),
    importPage.buttonImport.click(),
  ]);
  await preSubmissionPage.buttonBack.click();

  await preSubmissionPage.linkAddPoliticalGroup.click();
  await importPage.textfieldHashcode.fill(lastEventHash);
  await importPage.buttonImport.click();
  await expect(importPage.warningAlreadyImported).toBeVisible();

  await Promise.all([
    page.waitForURL(/\/csb\/pre-submission\/[^/]+$/),
    importPage.buttonImportAnyway.click(),
  ]);
  await expect(page.getByRole("heading", { name: groupName })).toBeVisible();
  await preSubmissionPage.buttonBack.click();
  await expect(preSubmissionPage.row(groupName)).toHaveCount(2);
});
