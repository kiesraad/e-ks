import type { Locator, Page } from "@playwright/test";
import type { Candidate } from "../../models/candidate";

export class CreatePersonPage {
  readonly textfieldInitials: Locator;
  readonly textfieldLastNamePrefix: Locator;
  readonly textfieldLastName: Locator;
  readonly textfieldFirstName: Locator;
  readonly textfieldCountryCode: Locator;
  readonly selectGender: Locator;
  readonly textfieldDayOfBirth: Locator;
  readonly textfieldMonthOfBirth: Locator;
  readonly textfieldYearOfBirth: Locator;
  readonly checkboxNoBSN: Locator;
  readonly buttonNext: Locator;
  readonly buttonSave: Locator;

  constructor(protected readonly page: Page) {
    this.textfieldInitials = this.page.getByLabel("Voorletters");
    this.textfieldLastNamePrefix = this.page.getByLabel("Voorvoegsel");
    this.textfieldLastName = this.page.getByLabel("Achternaam");
    this.textfieldFirstName = this.page.getByLabel("Roepnaam");
    this.textfieldCountryCode = this.page.getByRole("textbox", {
      name: "Landcode",
    });
    this.selectGender = this.page.getByLabel("Geslacht");

    this.textfieldDayOfBirth = this.page.getByRole("textbox", {
      name: "Geboortedatum",
    });
    this.textfieldMonthOfBirth = this.page.getByRole("textbox", {
      name: "Maand",
      exact: true,
    });
    this.textfieldYearOfBirth = this.page.getByRole("textbox", {
      name: "Jaar",
    });
    this.checkboxNoBSN = this.page.getByRole("checkbox", {
      name: "Ik bevestig dat deze persoon geen BSN heeft.",
    });
    this.buttonNext = this.page.getByRole("button", { name: "Volgende" });
    this.buttonSave = this.page.getByRole("button", { name: "Opslaan" });
  }

  async setPersonalDetails(candidate: Candidate) {
    await this.textfieldInitials.fill(candidate.initials);
    await this.textfieldLastNamePrefix.fill(candidate.lastNamePrefix ?? "");
    await this.textfieldLastName.fill(candidate.lastName);
    await this.textfieldFirstName.fill(candidate.firstName ?? "");
    if (candidate.countryCode) {
      await this.textfieldCountryCode.fill(candidate.countryCode);
    }
    await this.selectGender.selectOption(candidate.gender ?? "");

    await this.textfieldDayOfBirth.fill(candidate.dateOfBirth?.day ?? "");
    await this.textfieldMonthOfBirth.fill(candidate.dateOfBirth?.month ?? "");
    await this.textfieldYearOfBirth.fill(candidate.dateOfBirth?.year ?? "");

    await this.buttonNext.click();
  }
}
