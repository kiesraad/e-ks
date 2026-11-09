import { expect, type Locator, type Page } from "@playwright/test";
import type { Representative } from "../../models/representative";

export class RepresentativePage {
  readonly textfieldInitials: Locator;
  readonly textfieldLastNamePrefix: Locator;
  readonly textfieldLastName: Locator;
  readonly textfieldPostalCode: Locator;
  readonly textfieldHouseNumber: Locator;
  readonly textfieldHouseNumberAddition: Locator;
  readonly textfieldStreetName: Locator;
  readonly selectLocality: Locator;
  readonly buttonSaveAndClose: Locator;

  constructor(protected readonly page: Page) {
    this.textfieldInitials = this.page.getByLabel("Voorletters");
    this.textfieldLastNamePrefix = this.page.getByLabel("Voorvoegsel");
    this.textfieldLastName = this.page.getByLabel("Achternaam");
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
    this.buttonSaveAndClose = this.page.getByRole("button", {
      name: "Opslaan en sluiten",
    });
  }

  async setRepresentative(representative: Representative) {
    await this.textfieldInitials.fill(representative.initials);
    await this.textfieldLastNamePrefix.fill(
      representative.lastNamePrefix ?? "",
    );
    await this.textfieldLastName.fill(representative.lastName);
    await this.textfieldPostalCode.fill(representative.postalCode ?? "");
    await this.textfieldHouseNumber.fill(representative.houseNumber ?? "");
    await this.textfieldHouseNumberAddition.press("Tab");
    await expect(this.textfieldStreetName).toHaveValue(
      representative.streetName ?? "",
    );
    await expect(this.selectLocality).toHaveValue(
      representative.locality ?? "",
    );
    await this.buttonSaveAndClose.click();
  }
}
