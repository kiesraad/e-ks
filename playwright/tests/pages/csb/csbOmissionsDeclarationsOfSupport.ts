import { expect, type Locator, type Page } from "@playwright/test";

export class CsbOmissionsDeclarationsOfSupportPage {
  readonly headerDeclarationsOfSupport: Locator;
  readonly linkAdd: Locator;
  readonly linkOverview: Locator;
  readonly buttonMissingOneDistrict: Locator;
  readonly buttonMissingAllDistricts: Locator;
  readonly buttonMissingMultipleDistricts: Locator;
  readonly buttonDeclarationsNotValid: Locator;
  readonly textfieldTitle: Locator;
  readonly textfieldDescription: Locator;
  readonly textfieldLetter: Locator;
  readonly checkboxRecoverable: Locator;
  readonly buttonAddAndClose: Locator;
  readonly linkClose: Locator;
  private readonly buttonRemoveOmission: Locator;
  readonly checkboxAllLists: Locator;

  constructor(protected readonly page: Page) {
    this.headerDeclarationsOfSupport = this.page.getByRole("heading", {
      name: "Verzuimen - Ondersteuningsverklaringen",
    });
    this.linkAdd = this.page.getByRole("link", { name: "Verzuimen toevoegen" });
    this.linkOverview = this.page.getByRole("link", { name: "Overzicht" });
    this.buttonMissingOneDistrict = this.page.getByRole("button", {
      name: "Voor één kieskring ontbreken ondersteuningsverklaringen",
    });
    this.buttonMissingAllDistricts = this.page.getByRole("button", {
      name: "Voor alle kieskringen ontbreken ondersteuningsverklaringen",
    });
    this.buttonMissingMultipleDistricts = this.page.getByRole("button", {
      name: "Voor meerdere kieskringen ontbreken ondersteuningsverklaringen",
    });
    this.buttonDeclarationsNotValid = this.page.getByRole("button", {
      name: "Geen geldige ondersteuningsverklaringen ingeleverd",
    });
    this.textfieldTitle = this.page.getByRole("textbox", {
      name: "Titel Verzuim",
    });
    this.textfieldDescription = this.page.getByRole("textbox", {
      name: "I 1 verzuim toevoegen",
    });
    this.textfieldLetter = this.page.getByRole("textbox", {
      name: "Verzuimbriefnotitie toevoegen",
    });
    this.checkboxRecoverable = this.page.getByRole("checkbox", {
      name: "Herstelbaar verzuim",
    });
    this.buttonAddAndClose = this.page.getByRole("button", {
      name: "Toevoegen en sluiten",
    });
    this.linkClose = this.page.getByRole("link", { name: "Sluiten" });
    this.buttonRemoveOmission = this.page.getByRole("button", {
      name: "Verwijderen",
    });
    this.checkboxAllLists = this.page.getByRole("checkbox", {
      name: "Selecteer alle kandidatenlijsten",
    });
  }

  // verifies that the omission is only added for the selected electoral district
  async expectOnlySelectedDistrictAdded(
    page: Page,
    districts: string[],
    checkedDistrict: string,
  ) {
    for (const district of districts) {
      const text = page.getByText(district);
      if (district === checkedDistrict) {
        await expect(text).toBeVisible();
      } else {
        await expect(text).not.toBeVisible();
      }
    }
  }

  // verifies that the omission is added for all electoral districts
  async expectAllDistrictsAdded(page: Page, districts: string[]) {
    for (const district of districts) {
      const text = page.getByText(district);
      await expect(text).toBeVisible();
    }
  }

  async clickRemoveOmission() {
    // wait for the animation to finish before attempting click
    await this.page
      .locator(".animation")
      .evaluate((el) => Promise.all(el.getAnimations().map((a) => a.finished)));
    await this.buttonRemoveOmission.click();
  }
}
