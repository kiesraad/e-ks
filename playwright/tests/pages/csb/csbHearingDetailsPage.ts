import type { Locator, Page } from "@playwright/test";

export class CsbHearingDetailsPage {
readonly headerHearing: Locator;
readonly textfieldDateOfHearing: Locator;
readonly textfieldTimeOfHearing: Locator;
readonly textfieldNameOfChairperson: Locator;
readonly textfieldNameOfMember: Locator;
readonly buttonSave: Locator;

constructor(protected readonly page: Page) {
    this.headerHearing = this.page.getByRole("heading", {
      name: "Zitting details",
      exact: true,
    });
    this.textfieldDateOfHearing = this.page.getByLabel("Datum");
    this.textfieldTimeOfHearing = this.page.getByRole("textbox", { name: "Tijdstip zitting" });
    this.textfieldNameOfChairperson = this.page.getByRole("textbox", { name: "Naam voorzitter" });
    this.textfieldNameOfMember = this.page.getByRole("textbox", { name: "Naam lid" });
    this.buttonSave = this.page.getByRole("button", { name: "Opslaan" });
  }

  async selectPoliticalGroup(politicalgroup: string) {
    await this.page
      .getByRole("cell", { name: politicalgroup, exact: true })
      .click();
  }
}