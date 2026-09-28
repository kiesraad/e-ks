// Use case:
// Centraal stembureau vult de hascode in in e-KS

import { expect } from "@playwright/test";
import { test } from "./fixtures.ts";
import { CsbExaminationPage } from "./pages/csb/csbExaminationPage.ts";
import { CsbImportPage } from "./pages/csb/csbImportPage.ts";
import { CsbOverviewPage } from "./pages/csb/csbOverviewPage.ts";
import { CsbPoliticalGroupPage } from "./pages/csb/csbPoliticalGroupPage.ts";

test("import a political group in the CSB dashboard by hash", async ({
  csbLogin,
}) => {
  const { page, groupName, lastEventHash } = csbLogin;
  expect(lastEventHash).not.toBe("");

  const overviewPage = new CsbOverviewPage(page);
  const examinationPage = new CsbExaminationPage(page);
  const importPage = new CsbImportPage(page);
  const politicalGroupPage = new CsbPoliticalGroupPage(page);

  await expect(overviewPage.headerElection).toBeVisible();
  await overviewPage.linkExamination.click();

  await expect(examinationPage.headerExamination).toBeVisible();
  await examinationPage.linkAddPoliticalGroup.click();
  await expect(importPage.headerImport).toBeVisible();
  await importPage.textfieldHashcode.fill(lastEventHash);

  await Promise.all([
    page.waitForURL(/\/csb\/examination\/[^/]+/),
    page.getByRole("button", { name: "Importeren" }).click(),
  ]);

  await expect(page.getByRole("heading", { name: groupName })).toBeVisible();
  await politicalGroupPage.deleteGroup();
});

test("importing the same political group again warns and requires confirmation", async ({
  csbLogin,
}) => {
  const { page, groupName, lastEventHash } = csbLogin;
  expect(lastEventHash).not.toBe("");

  const overviewPage = new CsbOverviewPage(page);
  const examinationPage = new CsbExaminationPage(page);
  const importPage = new CsbImportPage(page);
  const politicalGroupPage = new CsbPoliticalGroupPage(page);

  await expect(overviewPage.headerElection).toBeVisible();
  await overviewPage.linkExamination.click();

  // first import succeeds directly
  await examinationPage.linkAddPoliticalGroup.click();
  await importPage.textfieldHashcode.fill(lastEventHash);
  await Promise.all([
    page.waitForURL(/\/csb\/examination\/[^/]+/),
    importPage.buttonImport.click(),
  ]);
  await politicalGroupPage.buttonBack.click();

  // importing the same hash again shows a warning instead of importing
  await examinationPage.linkAddPoliticalGroup.click();
  await importPage.textfieldHashcode.fill(lastEventHash);
  await importPage.buttonImport.click();
  await expect(importPage.warningAlreadyImported).toBeVisible();

  // confirming the warning imports the group a second time
  await Promise.all([
    page.waitForURL(/\/csb\/examination\/[^/]+/),
    importPage.buttonImportAnyway.click(),
  ]);
  await expect(page.getByRole("heading", { name: groupName })).toBeVisible();

  // clean up both imports
  await politicalGroupPage.deleteGroup();
  await examinationPage.selectPoliticalGroup(groupName);
  await politicalGroupPage.deleteGroup();
});
