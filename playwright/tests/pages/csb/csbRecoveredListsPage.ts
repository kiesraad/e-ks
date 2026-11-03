import type { Locator, Page } from "@playwright/test";

export class CsbRecoveredListsPage {
  readonly headerRecoveredLists: Locator;

  constructor(protected readonly page: Page) {
    this.headerRecoveredLists = this.page.getByRole("heading", {
      level: 1,
      name: "Verzuimperiode",
    });
  }

  async selectPoliticalGroup(politicalgroup: string) {
    await this.page
      .getByRole("cell", { name: politicalgroup, exact: true })
      .click();
  }
}
