import type { Locator, Page } from "@playwright/test";

export class CsbFinalizeExaminationPage {
  readonly linkHearingDetails: Locator;
  readonly headerExamination: Locator;
  readonly downloadI1pdf: Locator;
  readonly downloadI1word: Locator;
  readonly downloadOmissionLetters: Locator;

  constructor(protected readonly page: Page) {
    this.linkHearingDetails = this.page.getByRole("link", {
      name: "Zitting details",
    });
    this.headerExamination = this.page.getByRole("heading", {
      name: "Afronden onderzoek",
      exact: true,
    });
    this.downloadI1pdf = this.page.getByRole("link", { name: "Download PDF" });
    this.downloadI1word = this.page.getByRole("link", { name: "Download Word" });
    this.downloadOmissionLetters = this.page.getByRole("link", { name: "Download ZIP" });
  }

  async selectPoliticalGroup(politicalgroup: string) {
    await this.page
      .getByRole("cell", { name: politicalgroup, exact: true })
      .click();
  }
}
