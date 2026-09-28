// Use cases:
// De applicatie valideert dat alle benodigde gegevens correct zijn ingevuld.


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
import { ListDesignationPage } from "./pages/pg/listDesignationPage.ts";
import { ListSubmitter } from "./models/listSubmitter.ts";

test.describe("show warnings and errors for wrong input", async () => {
  test("general information", async ({ noExistingData: page }) => {
    const listDesignationPage = new ListDesignationPage(page);
    const politicalGroupPage = new PoliticalGroupPage(page);
    const listSubmittersPage = new ListSubmittersPage(page);
    const nameAuthorisationPage = new NameAuthorisationPage(page);
    const overviewPage = new OverviewPage(page);
    const authorisationIncorrect: NameAuthorisation = {
      initials: "A.B.C.D.E.F.G.H.I.J.K.",
      lastNamePrefix: "dee",
      lastName: "de Jansen",
      legalName: "TQW0iIAM1HTsThQ7TNyiZnpV8OjzN3HIbZrXJXWHkvO5Bc3134W7diX09c6msDmVGvlHuZ19WFAbPEFrm7VL0ztHRPiGZRPsf0v9QD5YFlAUhFeVaQuH1WRsvai1maCd1qc0ixfR00xXf2d2LhEj4A9KYkbYFKAyiRVQq6rJ8ntokWG1AvveqXBqYfWfiIG0BCfpcF42x",
    };
    const authorisationCorrect: NameAuthorisation = {
      initials: "A.B.C.",
      lastNamePrefix: "de",
      lastName: "Jansen",
      legalName: "TQW0iIAM1HTsThQ7TNyiZnpV8OjzN3HIbZrXJXWHkvO5Bc3134W7diX09c6msDmVGvlHuZ19WFAbPEFrm7VL0ztHRPiGZRPsf0v9QD5YFlAUhFeVaQuH1WRsvai1maCd1qc0ixfR00xXf2d2LhEj4A9KYkbYFKAyiRVQq6rJ8ntokWG1AvveqXBqYfWfiIG0BCfpcF42",
    };
    const authorisation2: NameAuthorisation = {
      initials: "A.",
      lastNamePrefix: "de",
      lastName: "Jansen",
      legalName: "test",
    };

    await page.goto("/political-group");
    await listDesignationPage.selectCombined.click();
    await listDesignationPage.buttonSaveAndNext.click();
    await politicalGroupPage.setCombinedDesignation("Partij lange geregistreerde aanduiding!");
    await politicalGroupPage.save();
    await expect(page.getByText("De waarde is te lang (36 tekens), maximaal 35 tekens toegestaan.")).toBeVisible();
    await politicalGroupPage.setCombinedDesignation("Partij lange geregistreerde aanduiding");
    await politicalGroupPage.save();
    await nameAuthorisationPage.addNameAuthorisation(authorisationIncorrect);
        const expectedErrors = [
      "De waarde is te lang (201 tekens), maximaal 200 tekens toegestaan.",
      "De waarde is te lang (22 tekens), maximaal 20 tekens toegestaan.",
      "De opgegeven waarde is ongeldig.",
      "Zet het voorvoegsel in het juiste veld.",
    ];

    for (const error of expectedErrors) {
      await expect(
        page.getByText(error),
      ).toBeVisible();
    }
    await nameAuthorisationPage.editNameAuthorisation([authorisationCorrect]);
    await expect(page.getByText("Combinaties van meerdere geregistreerde namen moeten uit minstens twee statutaire namen bestaan.")).toBeVisible();
    // to do: add step for identical statutory names
    await nameAuthorisationPage.addNameAuthorisation(authorisation2);
    await nameAuthorisationPage.buttonNext.click();
     const submitter: ListSubmitter = {
      initials: "R",
      lastNamePrefix: "de",
      lastName: "Vos",
        };
    await listSubmittersPage.setListSubmitter(submitter);
    
  });

  // test("candidates", async ({ login: page }) => {
  //   const finalisePage = new FinalisePage(page);
  //   const createPersonPage = new CreatePersonPage(page);

  //   await page.goto("/finalise");
  //   await finalisePage.linkBSN.click();
  //   await createPersonPage.checkboxNoBSN.check();
  //   await createPersonPage.buttonNext.click();
  //   await page.waitForURL("/finalise");
  //   await expect(finalisePage.linkBSN).not.toBeVisible();

  //   await finalisePage.linkIncorrectDate.first().click();
  //   await createPersonPage.textfieldYearOfBirth.fill("1925");
  //   await createPersonPage.buttonNext.click();
  //   await page.waitForURL("/finalise");

  //   await finalisePage.linkIncorrectDate.click();
  //   await createPersonPage.textfieldYearOfBirth.fill("1990");
  //   await createPersonPage.buttonNext.click();
  //   await page.waitForURL("/finalise");
  //   await expect(finalisePage.linkIncorrectDate).not.toBeVisible();
  // });

});
