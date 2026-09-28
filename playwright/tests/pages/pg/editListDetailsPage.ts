import type { Locator, Page } from "@playwright/test";

export class EditListDetailsPage {
  readonly buttonSave: Locator;
  readonly buttonClose: Locator;

  constructor(protected readonly page: Page) {
    this.buttonSave = this.page.getByRole("button", { name: "Opslaan" });
    this.buttonClose = this.page.getByRole("link", { name: "Sluiten" }).first();
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
}
