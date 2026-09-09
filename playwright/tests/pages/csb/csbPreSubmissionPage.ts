import type { Locator, Page } from "@playwright/test";

export class CsbPreSubmissionPage {
  readonly header: Locator;
  readonly linkAddPoliticalGroup: Locator;
  readonly headerErrors: Locator;
  readonly buttonBrpCheck: Locator;
  readonly buttonBack: Locator;

  constructor(protected readonly page: Page) {
    this.header = this.page.getByRole("heading", {
      name: "Voorinlevering",
      exact: true,
    });
    this.linkAddPoliticalGroup = this.page.getByRole("link", {
      name: "Politieke groepering toevoegen",
    });
    this.headerErrors = this.page.getByRole("heading", {
      name: "Ontdekte fouten",
    });
    this.buttonBrpCheck = this.page.getByRole("button", {
      name: "Controleren met de BRP",
    });
    this.buttonBack = this.page.getByRole("link", { name: "Terug" });
  }

  /// The table row of the group named `appellation`.
  row(appellation: string): Locator {
    return this.page.getByRole("row").filter({
      has: this.page.getByRole("cell", { name: appellation, exact: true }),
    });
  }

  async selectPoliticalGroup(appellation: string) {
    await this.page
      .getByRole("cell", { name: appellation, exact: true })
      .click();
  }
}
