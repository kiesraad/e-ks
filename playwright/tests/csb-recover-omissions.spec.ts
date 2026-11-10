// Use cases:
// CSB stelt vast dat de verzuimen zijn hersteld
// CSB zet in het systeem dat de verzuimen zijn hersteld
// De verzuimen zijn niet hersteld: CSB zet in het systeem dat verzuimen niet zijn hersteld
// De verzuimen zijn niet hersteld: CSB schrapt de kandidaat of de lijst (afhankelijk van het verzuim)

import { expect } from "@playwright/test";
import { test } from "./fixtures.ts";
import { CsbOmissionsDeclarationsOfSupportPage } from "./pages/csb/csbOmissionsDeclarationsOfSupport.ts";
import { CsbOmissionsPartyPage } from "./pages/csb/csbOmissionsPartyPage.ts";
import { CsbOverviewPage } from "./pages/csb/csbOverviewPage.ts";
import { CsbPoliticalGroupPage } from "./pages/csb/csbPoliticalGroupPage.ts";
import { CsbRecoveredListsPage } from "./pages/csb/csbRecoveredListsPage.ts";
import { CsbRecoveryGroupPage } from "./pages/csb/csbRecoveryGroupPage.ts";
import { CsbRecoveryOmissionsPage } from "./pages/csb/csbRecoveryOmissionsPage.ts";

test.describe("recover omissions", async () => {
  test("recoverable", async ({ csbImport }) => {
    const { page, groupName } = csbImport;
    const overviewPage = new CsbOverviewPage(page);
    const politicalGroupPage = new CsbPoliticalGroupPage(page);
    const omissionsPage = new CsbOmissionsDeclarationsOfSupportPage(page);
    const recoveredListsPage = new CsbRecoveredListsPage(page);
    const recoveryGroupPage = new CsbRecoveryGroupPage(page);
    const recoveryOmissionsPage = new CsbRecoveryOmissionsPage(page);

    // add omissions for two electoral districts
    await politicalGroupPage.selectedGroup(groupName);
    await politicalGroupPage.linkSupportDeclarations.click();
    await page.waitForURL(/\/omission\//);
    await expect(omissionsPage.headerDeclarationsOfSupport).toBeVisible();
    await page.getByRole("checkbox", { name: "1. Groningen" }).check();
    await page.getByRole("checkbox", { name: "2. Fryslân" }).check();
    await omissionsPage.buttonMissing.click();
    await omissionsPage.buttonAddAndClose.click();
    await politicalGroupPage.buttonHome.click();

    // recover omission for one electoral district and do not recover for other district, verify that only unrecovered list is scrapped
    await overviewPage.linkRecoveredLists.click();
    await expect(recoveredListsPage.headerRecoveredLists).toBeVisible();
    await recoveredListsPage.selectPoliticalGroup(groupName);
    await recoveryGroupPage.selectedGroup(groupName);
    await expect(page.getByText("Nog 2 te beoordelen")).toBeVisible();
    // the declarations of support are decided on the omissions page, in one row
    await recoveryGroupPage.buttonRecoverOmissions.click();
    await expect(recoveryOmissionsPage.headerRecoveryOmissions).toBeVisible();
    await expect(
      page.getByText(
        "Voor meerdere kieskringen ontbreken ondersteuningsverklaringen",
      ),
    ).toBeVisible();
    await recoveryGroupPage.recoverOmission("1. Groningen", true);
    await recoveryGroupPage.recoverOmission("2. Fryslân", false);
    await expect(page.locator("li.omission-status-row")).toHaveCount(1);
    await recoveryOmissionsPage.buttonBack.click();
    await recoveryGroupPage.selectedGroup(groupName);
    await expect(page.getByText("Nog 0 te beoordelen")).toBeVisible();
    await expect(
      page
        .locator(".examination-panel")
        .filter({ has: page.getByRole("heading", { name: "Geschrapt" }) })
        .getByRole("link", { name: "2. Fryslân" }),
    ).toBeVisible();
    await expect(
      page
        .locator(".examination-panel")
        .filter({ has: page.getByRole("heading", { name: "Geschrapt" }) })
        .getByRole("link", { name: "1. Groningen" }),
    ).not.toBeVisible();
  });

  test("unrecoverable", async ({ csbImport }) => {
    const { page, groupName } = csbImport;
    const overviewPage = new CsbOverviewPage(page);
    const politicalGroupPage = new CsbPoliticalGroupPage(page);
    const omissionsPage = new CsbOmissionsPartyPage(page);
    const recoveredListsPage = new CsbRecoveredListsPage(page);
    const recoveryGroupPage = new CsbRecoveryGroupPage(page);

    // add omission for non registered appellation
    await politicalGroupPage.selectedGroup(groupName);
    await politicalGroupPage.linkAddAppellationOmission.click();
    await page.waitForURL(/\/omission\//);
    await omissionsPage.buttonRegisterAppellation.click();
    await omissionsPage.buttonAddAndClose.click();
    await politicalGroupPage.buttonHome.click();

    // verify omission is unrecoverable,
    // appellation is scrapped but should still shows original appellation
    await overviewPage.linkRecoveredLists.click();
    await expect(recoveredListsPage.headerRecoveredLists).toBeVisible();
    await recoveredListsPage.selectPoliticalGroup(groupName);
    await recoveryGroupPage.selectedGroup(groupName);
    await expect(
      page
        .locator(".examination-panel")
        .filter({ has: page.getByRole("heading", { name: "Geschrapt" }) })
        .getByRole("link", { name: groupName }),
    ).toBeVisible();
    await recoveryGroupPage.buttonRecoverOmissions.click();
    await expect(
      page.getByText("De aanduiding is niet geregistreerd"),
    ).toBeVisible();
    await expect(
      page.getByText("Onherstelbaar", { exact: true }),
    ).toBeVisible();
  });
});
