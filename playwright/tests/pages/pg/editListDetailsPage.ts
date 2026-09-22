import type { Locator, Page } from "@playwright/test";

export class EditListDetailsPage {
  readonly buttonSave: Locator;
  readonly buttonClose: Locator;
  readonly buttonRemoveList: Locator;
  readonly buttonConfirmRemoveList: Locator;

  constructor(protected readonly page: Page) {
    this.buttonSave = this.page.getByRole("button", { name: "Opslaan" });
    this.buttonClose = this.page.getByRole("link", { name: "Sluiten" }).first();
    this.buttonRemoveList = this.page.getByRole("button", {
      name: "Kandidatenlijst verwijderen",
    });
    this.buttonConfirmRemoveList = this.page.getByRole("button", {
      name: "Verwijderen",
      exact: true,
    });
  }

  async addDistricts(districts: string[]) {
    for (const district of districts) {
      await this.page.getByRole("checkbox", { name: district }).check();
    }
    await this.buttonSave.click();
  }

  async removeDistricts(districts: string[]) {
    for (const district of districts) {
      await this.page.getByRole("checkbox", { name: district }).uncheck();
    }
    await this.buttonSave.click();
  }

  async removeList() {
    await this.buttonRemoveList.click();
    await this.buttonConfirmRemoveList.click();
  }
}
