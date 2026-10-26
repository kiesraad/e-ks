import type { Locator, Page } from "@playwright/test";
import type { NameAuthorisation } from "../../models/nameAuthorisation";

export class NameAuthorisationPage {
  readonly textfieldLegalName: Locator;
  readonly textfieldInitials: Locator;
  readonly textfieldLastNamePrefix: Locator;
  readonly textfieldLastName: Locator;
  readonly buttonDelete: Locator;
  readonly buttonConfirmDelete: Locator;
  readonly buttonAdd: Locator;
  readonly buttonSave: Locator;
  readonly buttonNext: Locator;

  constructor(protected readonly page: Page) {
    this.textfieldLegalName = this.page.getByLabel("Volledige statutaire naam");
    this.textfieldInitials = this.page.getByLabel("Voorletters");
    this.textfieldLastNamePrefix = this.page.getByLabel("Voorvoegsel");
    this.textfieldLastName = this.page.getByLabel("Achternaam");
    this.buttonDelete = this.page.getByRole("link", {
      name: "Machtiging verwijderen",
      exact: true,
    });
    this.buttonConfirmDelete = this.page.getByRole("button", {
      name: "Machtiging verwijderen",
      exact: true,
    });
    this.buttonAdd = this.page.getByRole("link", {
      name: "Statutaire naam toevoegen",
    });
    this.buttonSave = this.page.getByRole("button", { name: "Opslaan" });
    this.buttonNext = this.page.getByRole("link", {
      name: "Opslaan en volgende",
    });
  }

  getAgentLocator(lastName: string) {
    return this.page.getByRole("link", { name: new RegExp(lastName) });
  }

  async deleteExistingAuthorisedAgents() {
    //takes all links from table and saves href attributes of each link in list
    const hrefs = await this.page
      .locator(".person-block")
      .evaluateAll((links) => links.map((link) => link.getAttribute("href")));

    for (const href of hrefs) {
      if (href) {
        await this.page.goto(href);
        await this.buttonDelete.click();
        await this.buttonConfirmDelete.click();
        await this.page.waitForURL("**/name-authorisation");
      }
    }
  }

  async addNameAuthorisation(nameAuthorisation: NameAuthorisation) {
    await this.buttonAdd.click();
    await this.textfieldLegalName.fill(nameAuthorisation.legalName);
    await this.textfieldInitials.fill(nameAuthorisation.initials);
    await this.textfieldLastNamePrefix.fill(
      nameAuthorisation.lastNamePrefix ?? "",
    );
    await this.textfieldLastName.fill(nameAuthorisation.lastName);
    await this.buttonSave.click();
  }

  async editNameAuthorisation(nameAuthorisations: NameAuthorisation[]) {
    for (const nameAuthorisation of nameAuthorisations) {
      await this.textfieldLegalName.fill(nameAuthorisation.legalName);
      await this.textfieldInitials.fill(nameAuthorisation.initials);
      await this.textfieldLastNamePrefix.fill(
        nameAuthorisation.lastNamePrefix ?? "",
      );
      await this.textfieldLastName.fill(nameAuthorisation.lastName);
      await this.buttonSave.click();
    }
  }
}
