import type { Locator, Page } from "@playwright/test";

export class CsbHearingDetailsPage {
  readonly headerHearing: Locator;
  readonly textfieldTimeOfHearing: Locator;
  readonly textfieldNameOfChairperson: Locator;
  readonly textfieldNameOfMember: Locator;
  readonly buttonSave: Locator;

  constructor(protected readonly page: Page) {
    this.headerHearing = this.page.getByRole("heading", {
      name: "Zitting details",
      exact: true,
    });
    this.textfieldTimeOfHearing = this.page.getByRole("textbox", {
      name: "Tijdstip zitting",
    });
    this.textfieldNameOfChairperson = this.page.getByRole("textbox", {
      name: "Naam voorzitter",
    });
    this.textfieldNameOfMember = this.page.getByRole("textbox", {
      name: "Naam lid",
    });
    this.buttonSave = this.page.getByRole("button", { name: "Opslaan" });
  }

  async selectPoliticalGroup(politicalgroup: string) {
    await this.page
      .getByRole("cell", { name: politicalgroup, exact: true })
      .click();
  }

  async setDateOfHearing(day: string, month: string, year: string) {
    await this.page.getByRole("textbox", { name: "Datum zitting" }).fill(day);
    await this.page.getByRole("textbox", { name: "Maand" }).fill(month);
    await this.page.getByRole("textbox", { name: "Jaar" }).fill(year);
  }
}
