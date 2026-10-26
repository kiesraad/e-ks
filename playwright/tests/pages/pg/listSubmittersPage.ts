import type { Locator, Page } from "@playwright/test";
import type { ListSubmitter } from "../../models/listSubmitter";

export class ListSubmittersPage {
  readonly buttonAdd: Locator;
  readonly buttonSave: Locator;
  readonly textfieldInitials: Locator;
  readonly textfieldLastNamePrefix: Locator;
  readonly textfieldLastName: Locator;
  readonly textfieldPostalCode: Locator;
  readonly textfieldHouseNumber: Locator;
  readonly textfieldHouseNumberAddition: Locator;
  readonly textfieldStreetName: Locator;
  readonly textfieldLocality: Locator;
  readonly linkEditSubmitter: Locator;
  readonly buttonNext: Locator;

  constructor(protected readonly page: Page) {
    this.buttonAdd = this.page.getByRole("link", {
      name: "Lijstinleveraar toevoegen",
    });
    this.buttonSave = this.page.getByRole("button", { name: "Opslaan" });
    this.textfieldInitials = this.page.getByLabel("Voorletters");
    this.textfieldLastNamePrefix = this.page.getByLabel("Voorvoegsel");
    this.textfieldLastName = this.page.getByLabel("Achternaam");
    this.textfieldPostalCode = this.page.getByLabel("Postcode");
    this.textfieldHouseNumber = this.page.getByLabel("Huisnummer", {
      exact: true,
    });
    this.textfieldHouseNumberAddition = this.page.getByLabel(
      "Huisnummer toevoeging",
    );
    this.textfieldStreetName = this.page.getByLabel("Straatnaam");
    this.textfieldLocality = this.page.getByLabel("Woonplaats");
    // On the view page the single list submitter is rendered as a
    // .person-block link that opens the edit overlay.
    this.linkEditSubmitter = this.page.locator("a.person-block").first();
    this.buttonNext = this.page.getByRole("link", {
      name: "Opslaan en naar overzicht",
    });
  }

  getSubmitterLocator(lastName: string) {
    return this.page.getByRole("link", { name: new RegExp(lastName) });
  }

  async openEditor() {
    // Prefer the "add" button when the submitter is still empty,
    // otherwise click the existing submitter block to edit it.
    if (await this.buttonAdd.isVisible()) {
      await this.buttonAdd.click();
    } else {
      await this.linkEditSubmitter.click();
    }
  }

  async setListSubmitter(listSubmitter: ListSubmitter) {
    await this.openEditor();
    await this.textfieldInitials.fill(listSubmitter.initials);
    await this.textfieldLastNamePrefix.fill(listSubmitter.lastNamePrefix ?? "");
    await this.textfieldLastName.fill(listSubmitter.lastName);
    await this.textfieldPostalCode.fill(listSubmitter.postalCode ?? "");
    await this.textfieldHouseNumber.fill(listSubmitter.houseNumber ?? "");
    await this.textfieldHouseNumberAddition.fill(
      listSubmitter.houseNumberAddition ?? "",
    );
    await this.textfieldStreetName.fill(listSubmitter.streetName ?? "");
    await this.textfieldLocality.fill(listSubmitter.locality ?? "");
    await this.buttonSave.click();

    await this.page.waitForURL("/political-group/list-submitter*");
  }
}
