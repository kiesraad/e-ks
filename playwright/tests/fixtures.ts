import { test as base, type Page } from "@playwright/test";
import { CsbExaminationPage } from "./pages/csb/csbExaminationPage.ts";
import { CsbImportPage } from "./pages/csb/csbImportPage.ts";
import { CsbOverviewPage } from "./pages/csb/csbOverviewPage.ts";
import { CandidateListsOverviewPage } from "./pages/pg/candidateListsOverviewPage.ts";
import { ManageCandidateListPage } from "./pages/pg/manageCandidateListPage.ts";
import { SelectElectionPage } from "./pages/pg/selectElectionPage.ts";

type CsbLogin = {
  page: Page;
  groupName: string;
  lastEventHash: string;
};

type Fixtures = {
  login: Page;
  noExistingData: Page;
  deleteExistingCandidateLists: Page;
  provincialCouncilElection: Page;
  provincialCouncilFrisianElection: Page;
  waterAuthorityElection: Page;
  csbLogin: CsbLogin;
  csbImport: CsbLogin;
  csbOnlyImport: CsbLogin;
};

export const test = base.extend<Fixtures>({
  login: async ({ page }, use) => {
    await page.goto("/dev/login?fixtures=true");
    await use(page);
  },

  noExistingData: async ({ page }, use) => {
    await page.goto("/dev/login?fixtures=false");
    await use(page);
  },

  // Load fixtures into a fresh political-group stream with a unique name and
  // capture the chain hash of its last event, then log in as CSB. The hash can
  // be entered on the CSB import page to import the group.
  csbLogin: async ({ page }, use) => {
    const groupName = `Test Partij ${Math.random().toString(36).slice(2, 10)}`;
    const response = await page.request.get(
      `/dev/login?fixtures=true&name=${encodeURIComponent(groupName)}`,
      { maxRedirects: 0 },
    );
    const lastEventHash = response.headers()["x-last-event-hash"] ?? "";
    await page.goto("/dev/login?csb=true");
    await use({ page, groupName, lastEventHash });
  },

  // Login as CSB and import a political group with a unique name. Deletes group after test is done
  csbImport: async ({ csbLogin }, use) => {
    const { page, groupName, lastEventHash } = csbLogin;
    const overviewPage = new CsbOverviewPage(page);
    const examinationPage = new CsbExaminationPage(page);
    const importPage = new CsbImportPage(page);
    await overviewPage.linkExamination.click();
    await examinationPage.linkAddPoliticalGroup.click();
    await importPage.textfieldHashcode.fill(lastEventHash);
    await Promise.all([
      page.waitForURL(/\/csb\/examination\/[^/]+/),
      page.getByRole("button", { name: "Importeren" }).click(),
    ]);
    await use({ page, groupName, lastEventHash });
    await page.goto("/csb/examination");
    await page.getByRole("cell", { name: groupName }).click();
    await page.getByRole("link", { name: "Verwijderen" }).click();
    await page.getByRole("button", { name: "Verwijderen" }).click();
    // wait for the redirect to the overview, so the fixture does not tear
    // down while the delete request is still in flight
    await page.waitForURL(/\/csb\/examination$/);
  },

  // Login as CSB and import a political group with a unique name, but no cleanup after
  csbOnlyImport: async ({ csbLogin }, use) => {
    const { page, groupName, lastEventHash } = csbLogin;
    const overviewPage = new CsbOverviewPage(page);
    const examinationPage = new CsbExaminationPage(page);
    const importPage = new CsbImportPage(page);
    await overviewPage.linkExamination.click();
    await examinationPage.linkAddPoliticalGroup.click();
    await importPage.textfieldHashcode.fill(lastEventHash);
    await Promise.all([
      page.waitForURL(/\/csb\/examination\/[^/]+/),
      page.getByRole("button", { name: "Importeren" }).click(),
    ]);
    await use({ page, groupName, lastEventHash });
  },

  deleteExistingCandidateLists: async ({ page }, use) => {
    await page.goto(`/dev/login?fixtures=true`);
    await page.goto("/candidate-lists");
    const candidateListsOverviewPage = new CandidateListsOverviewPage(page);

    const hrefs =
      await candidateListsOverviewPage.linkCandidateList.evaluateAll((links) =>
        links.map((link) => link.getAttribute("href")),
      );

    for (const href of hrefs) {
      if (href) {
        await page.goto(href);
        await new ManageCandidateListPage(page).removeList();
        await new CandidateListsOverviewPage(page).buttonAddList.waitFor();
      }
    }

    await use(page);
  },

  provincialCouncilElection: async ({ page }, use) => {
    await page.goto("/dev/login?select_election=true");
    const selectElectionPage = new SelectElectionPage(page);
    await selectElectionPage.dropdownElections.selectOption("PS27");
    await selectElectionPage.dropdownProvinces.selectOption("Noord-Holland");
    await selectElectionPage.checkboxFixtures.check();
    await Promise.all([
      page.waitForURL("/"),
      selectElectionPage.buttonContinue.click(),
    ]);

    await use(page);
  },

  provincialCouncilFrisianElection: async ({ page }, use) => {
    await page.goto("/dev/login?select_election=true");
    const selectElectionPage = new SelectElectionPage(page);
    await selectElectionPage.dropdownElections.selectOption("PS27");
    await selectElectionPage.dropdownProvinces.selectOption("Fryslân");
    await selectElectionPage.checkboxFixtures.check();
    await Promise.all([
      page.waitForURL("/"),
      selectElectionPage.buttonContinue.click(),
    ]);

    await use(page);
  },

  waterAuthorityElection: async ({ page }, use) => {
    await page.goto("/dev/login?select_election=true");
    const selectElectionPage = new SelectElectionPage(page);
    await selectElectionPage.dropdownElections.selectOption(
      "Waterschapsverkiezingen 2027",
    );
    await selectElectionPage.dropdownWaterAuthorities.selectOption(
      "Amstel, Gooi en Vecht",
    );
    await selectElectionPage.checkboxFixtures.uncheck();
    await Promise.all([
      page.waitForURL("/"),
      selectElectionPage.buttonContinue.click(),
    ]);

    await use(page);
  },
});
