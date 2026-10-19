// Use cases:
// CSB stelt vast dat de verzuimen zijn hersteld
// CSB zet in het systeem dat de verzuimen zijn hersteld
// De verzuimen zijn niet hersteld: CSB zet in het systeem dat verzuimen niet zijn hersteld
// De verzuimen zijn niet hersteld: CSB schrapt de kandidaat of de lijst (afhankelijk van het verzuim)

import { expect } from "@playwright/test";
import { test } from "./fixtures.ts";
import { CsbGeneralInformationPage } from "./pages/csb/csbGeneralInformationPage.ts";
import { CsbOmissionsDeclarationsOfSupportPage } from "./pages/csb/csbOmissionsDeclarationsOfSupport.ts";
import { CsbOmissionsPartyPage } from "./pages/csb/csbOmissionsPartyPage.ts";
import { CsbOverviewPage } from "./pages/csb/csbOverviewPage.ts";
import { CsbPoliticalGroupPage } from "./pages/csb/csbPoliticalGroupPage.ts";
import { CsbRecoveredListsPage } from "./pages/csb/csbRecoveredListsPage.ts";
import { CsbRecoveryGroupPage } from "./pages/csb/csbRecoveryGroupPage.ts";

test.describe("recover omissions", async () => {
  test("recoverable", async ({ csbImport }) => {
    const { page, groupName } = csbImport;
    const overviewPage = new CsbOverviewPage(page);
    const politicalGroupPage = new CsbPoliticalGroupPage(page);
    const omissionsPage = new CsbOmissionsDeclarationsOfSupportPage(page);
    const recoveredListsPage = new CsbRecoveredListsPage(page);
    const recoveryGroupPage = new CsbRecoveryGroupPage(page);

    // add omissions for two electoral districts
    await politicalGroupPage.selectedGroup(groupName);
    await politicalGroupPage.linkSupportDeclarations.click();
    await page.waitForURL(/\/omission\//);
    await expect(omissionsPage.headerDeclarationsOfSupport).toBeVisible();
    await page.getByRole("checkbox", { name: "1. Groningen" }).check();
    await page.getByRole("checkbox", { name: "2. Fryslân" }).check();
    await omissionsPage.buttonMissingMultipleDistricts.click();
    await omissionsPage.buttonAddAndClose.click();
    await politicalGroupPage.buttonHome.click();

    // recover omission for one electoral district and do not recover for other district, verify that only unrecovered list is scrapped
    await overviewPage.linkRecoveredLists.click();
    await expect(recoveredListsPage.headerRecoveredLists).toBeVisible();
    await recoveredListsPage.selectPoliticalGroup(groupName);
    await recoveryGroupPage.selectedGroup(groupName);
    await expect(
      page.getByText(
        "Voor meerdere kieskringen ontbreken ondersteuningsverklaringen",
      ),
    ).toBeVisible();
    await recoveryGroupPage.recoverOmission("1. Groningen", true);
    await recoveryGroupPage.recoverOmission("2. Fryslân", false);
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
    const generalInformationPage = new CsbGeneralInformationPage(page);

    // add omission for non registered appellation
    await politicalGroupPage.selectedGroup(groupName);
    await politicalGroupPage.linkGeneralInformation.click();
    await generalInformationPage.linkAddAppellationOmission.click();
    await page.waitForURL(/\/omission\//);
    await omissionsPage.buttonRegisterAppelation.click();
    await omissionsPage.buttonAddAndClose.click();
    await politicalGroupPage.buttonHome.click();

    // verify omission is unrecoverable, appellation is scrapped and list is now blank
    await overviewPage.linkRecoveredLists.click();
    await expect(recoveredListsPage.headerRecoveredLists).toBeVisible();
    await recoveredListsPage.selectPoliticalGroup("Blanco (de Goede, Z.)");
    await recoveryGroupPage.selectedGroup("Blanco (de Goede, Z.)");
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
