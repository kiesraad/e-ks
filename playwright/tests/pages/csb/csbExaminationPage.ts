import type { Locator, Page } from "@playwright/test";

export class CsbExaminationPage {
  readonly buttonFinalise: Locator;
  readonly linkAddPoliticalGroup: Locator;
  readonly headerExamination: Locator;

  constructor(protected readonly page: Page) {
    this.buttonFinalise = this.page.getByRole("link", {
      name: "Afronden",
    });
    this.linkAddPoliticalGroup = this.page.getByRole("link", {
      name: "Politieke groepering toevoegen",
    });
    this.headerExamination = this.page.getByRole("heading", {
      name: "Onderzoek",
      exact: true,
    });
  }

  async selectPoliticalGroup(politicalgroup: string) {
    await this.page
      .getByRole("cell", { name: politicalgroup, exact: true })
      .click();
  }
}
