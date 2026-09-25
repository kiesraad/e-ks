import { expect } from "@playwright/test";
import { test } from "./fixtures.ts";
import { CsbAllRestorationsPage } from "./pages/csb/csbAllRestorationsPage.ts";
import { CsbPoliticalGroupPage } from "./pages/csb/csbPoliticalGroupPage.ts";
import { CsbRecoveryOmissionsPage } from "./pages/csb/csbRecoveryOmissionsPage.ts";

test.describe("omissions overview pages do not link to themselves", async () => {
  test("all restorations page in examination", async ({ csbImport }) => {
    const { page, groupName } = csbImport;
    const politicalGroupPage = new CsbPoliticalGroupPage(page);
    const allRestorationsPage = new CsbAllRestorationsPage(page);

    await politicalGroupPage.selectedGroup(groupName);
    await politicalGroupPage.linkRectifications.click();

    await expect(allRestorationsPage.headerAllRestorations).toBeVisible();
    await expect(allRestorationsPage.buttonBack).toBeVisible();
    await expect(allRestorationsPage.buttonAllRestorations).toHaveCount(0);
  });

  test("omissions page in recovery", async ({ csbImport }) => {
    const { page } = csbImport;
    const recoveryOmissionsPage = new CsbRecoveryOmissionsPage(page);
    const streamId = new URL(page.url()).pathname.split("/").pop();

    await page.goto(`/csb/recovery/${streamId}/omissions`);

    await expect(recoveryOmissionsPage.headerRecoveryOmissions).toBeVisible();
    await expect(recoveryOmissionsPage.buttonBack).toBeVisible();
    await expect(recoveryOmissionsPage.buttonRecoverOmissions).toHaveCount(0);
  });
});
