import { expect, type Locator, type Page } from "@playwright/test";
import type { Candidate } from "../../models/candidate";

export class CorrespondenceAddressPage {
  readonly textfieldPostalCode: Locator;
  readonly textfieldHouseNumber: Locator;
  readonly textfieldHouseNumberAddition: Locator;
  readonly textfieldStreetName: Locator;
  readonly selectLocality: Locator;
  readonly buttonCloseAndSave: Locator;

  constructor(protected readonly page: Page) {
    this.textfieldPostalCode = this.page.getByRole("textbox", {
      name: "Postcode",
    });
    this.textfieldHouseNumber = this.page.getByRole("textbox", {
      name: "Huisnummer",
      exact: true,
    });
    this.textfieldHouseNumberAddition = this.page.getByRole("textbox", {
      name: "Huisnummer toevoeging",
      exact: true,
    });
    this.textfieldStreetName = this.page.getByRole("textbox", {
      name: "Straatnaam",
    });
    this.selectLocality = this.page.getByRole("textbox", {
      name: "Woonplaats",
    });
    this.buttonCloseAndSave = this.page.getByRole("button", {
      name: "Opslaan en sluiten",
    });
  }

  async setCorrespondenceAddress(candidate: Candidate) {
    await this.textfieldPostalCode.fill(candidate.postalCode ?? "");
    await this.textfieldHouseNumber.pressSequentially(
      candidate.houseNumber ?? "",
    );
    await this.textfieldHouseNumberAddition.pressSequentially(
      candidate.houseNumberAddition ?? "",
    );
    await expect(this.textfieldStreetName).toHaveValue(
      candidate.streetName ?? "",
    );
    await expect(this.selectLocality).toHaveValue(candidate.locality ?? "");

    await this.buttonCloseAndSave.click();
  }
}
