import { expect, type Locator, type Page } from "@playwright/test";

export class CsbOmissionsCandidatePage {
  readonly linkAdd: Locator;
  readonly linkOverview: Locator;
  readonly buttonWrongAppelation: Locator;
  readonly buttonWrongGender: Locator;
  readonly buttonDifferenceH1: Locator;
  readonly buttonAuthorisedPerson: Locator;
  readonly buttonMissingSupportDeclaration: Locator;
  readonly buttonMissingCopyID: Locator;
  readonly buttonMissingAddress: Locator;
  readonly buttonDifferentSignature: Locator;
  readonly buttonIncorrectSignature: Locator;
  readonly buttonIncorrectDate: Locator;
  readonly buttonMissingDate: Locator;
  readonly buttonMissingSignature: Locator;
  readonly textfieldTitle: Locator;
  readonly textfieldDescription: Locator;
  readonly textfieldLetter: Locator;
  readonly checkboxRecoverable: Locator;
  readonly buttonAddAndClose: Locator;
  readonly linkClose: Locator;
  private readonly buttonRemoveOmission: Locator;
  readonly checkboxAllLists: Locator;

  constructor(protected readonly page: Page) {
    this.linkAdd = this.page.getByRole("link", { name: "Verzuimen toevoegen" });
    this.linkOverview = this.page.getByRole("link", { name: "Overzicht" });
    this.buttonWrongAppelation = this.page.getByRole("button", {
      name: "Kandidaat onjuist vermeld",
    });
    this.buttonWrongGender = this.page.getByRole("button", {
      name: "Bij kandidaat is het onjuiste geslacht (x) vermeld",
    });
    this.buttonDifferenceH1 = this.page.getByRole("button", {
      name: "De kandidatenlijst op Model H 1 en op de instemmingsverklaring komen niet overeen",
    });
    this.buttonAuthorisedPerson = this.page.getByRole("button", {
      name: "Gemachtigde kandidaat ontbreekt",
    });
    this.buttonMissingSupportDeclaration = this.page.getByRole("button", {
      name: "Verklaring en kopie ID ontbreken",
    });
    this.buttonMissingCopyID = this.page.getByRole("button", {
      name: "Kopie ID ontbreekt",
    });
    this.buttonMissingAddress = this.page.getByRole("button", {
      name: "Geen (volledig) adres opgegeven op de instemmingsverklaring",
    });
    this.buttonIncorrectSignature = this.page.getByRole("button", {
      name: "De handtekening onder de instemmingsverklaring is niet correct",
    });
    this.buttonDifferentSignature = this.page.getByRole("button", {
      name: "De handtekening onder de instemmingsverklaring en op het kopie ID komen niet overeen",
    });
    this.buttonIncorrectDate = this.page.getByRole("button", {
      name: "Datum van ondertekening is niet correct",
    });
    this.buttonMissingDate = this.page.getByRole("button", {
      name: "Datum ondertekening ontbreekt",
    });
    this.buttonMissingSignature = this.page.getByRole("button", {
      name: "Handtekening ontbreekt",
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

  // verifies that the electoral district for the selected list is checked and districts for other lists are not checked
  async expectOnlySelectedDistrictChecked(
    page: Page,
    districts: string[],
    checkedDistrict: string,
  ) {
    for (const district of districts) {
      const checkbox = page.getByRole("checkbox", { name: district });
      if (district === checkedDistrict) {
        await expect(checkbox).toBeChecked();
      } else {
        await expect(checkbox).not.toBeChecked();
      }
    }
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

  // verifies that the omission is added for the given electoral districts only
  async expectDistrictsAdded(
    page: Page,
    added: string[],
    notAdded: string[],
  ) {
    for (const district of added) {
      await expect(page.getByText(district)).toBeVisible();
    }
    for (const district of notAdded) {
      await expect(page.getByText(district)).not.toBeVisible();
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
