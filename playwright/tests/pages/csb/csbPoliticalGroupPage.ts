import { expect, type Locator, type Page } from "@playwright/test";

export class CsbPoliticalGroupPage {
  readonly switchFinalize: Locator;
  readonly linkRectifications: Locator;
  readonly buttonBack: Locator;
  readonly linkAllErrors: Locator;
  readonly buttonPaperCorrections: Locator;
  readonly linkGeneralInformation: Locator;
  readonly linkSupportDeclarations: Locator;
  readonly linkSupportDeclarationsOverview: Locator;
  readonly linkCandidateList: Locator;
  readonly linkDelete: Locator;
  readonly buttonDeleteConfirm: Locator;
  readonly buttonBRPCheck: Locator;
  readonly linkRefresh: Locator;
  readonly buttonHome: Locator;

  constructor(protected readonly page: Page) {
    this.switchFinalize = this.page.getByLabel(
      "Onderzoek afronden of heropenen",
    );
    this.linkRectifications = this.page.getByRole("link", {
      name: "Verzuimen en correcties",
    });
    this.buttonBack = this.page.getByRole("link", {
      name: "Terug",
    });
    this.linkAllErrors = this.page.getByRole("link", {
      name: "Naar alle fouten",
    });
    this.buttonPaperCorrections = this.page.getByRole("button", {
      name: "Ga naar gegevens",
    });
    this.linkGeneralInformation = this.page.getByRole("link", {
      name: "Basisgegevens controleren",
    });
    this.linkSupportDeclarations = this.page.getByRole("link", {
      name: "Verzuim toevoegen",
    });
    this.linkSupportDeclarationsOverview = this.page.getByRole("link", {
      name: "Overzicht",
    });
    this.linkCandidateList = this.page.getByRole("link", {
      name: "Kandidatenlijst controleren",
    });
    this.linkDelete = this.page.getByRole("link", { name: "Verwijderen" });
    this.buttonDeleteConfirm = this.page.getByRole("button", {
      name: "Verwijderen",
    });
    this.buttonBRPCheck = this.page.getByRole("button", {
      name: "Controleren met de BRP",
    });
    this.linkRefresh = this.page.getByRole("link", {
      name: "Pagina verversen",
    });
    this.buttonHome = this.page.getByLabel("Start", {
      exact: true,
    });
  }

  async selectedGroup(politicalgroup: string) {
    await expect(
      this.page.getByRole("heading", { name: politicalgroup }),
    ).toBeVisible();
  }

  async deleteGroup() {
    await this.linkDelete.click();
    await this.buttonDeleteConfirm.click();
    // wait for the redirect to the overview, so the test does not end while
    // the delete request is still in flight
    await this.page.waitForURL(/\/csb\/examination$/);
  }
}
