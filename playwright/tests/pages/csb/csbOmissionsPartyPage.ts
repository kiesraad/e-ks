import type { Locator, Page } from "@playwright/test";

export class CsbOmissionsPartyPage {
  readonly linkAdd: Locator;
  readonly linkOverview: Locator;
  readonly buttonRegisterAppelation: Locator;
  readonly buttonRegisterCombination: Locator;
  readonly textfieldTitle: Locator;
  readonly textfieldDescription: Locator;
  readonly textfieldLetter: Locator;
  readonly checkboxRecoverable: Locator;
  readonly buttonAddAndClose: Locator;
  readonly linkClose: Locator;
  readonly noLetterWarning: Locator;
  private readonly buttonRemoveOmission: Locator;

  constructor(protected readonly page: Page) {
    this.linkAdd = this.page.getByRole("link", { name: "Verzuimen toevoegen" });
    this.linkOverview = this.page.getByRole("link", { name: "Overzicht" });
    this.buttonRegisterAppelation = this.page.getByRole("button", {
      name: "De aanduiding is niet geregistreerd",
    });
    this.buttonRegisterCombination = this.page.getByRole("button", {
      name: "De aanduiding(en) is/zijn niet geregistreerd",
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
    this.noLetterWarning = this.page.locator(
      "[data-omission-no-letter-warning]",
    );
    this.buttonRemoveOmission = this.page.getByRole("button", {
      name: "Verwijderen",
    });
  }

  async clickRemoveOmission() {
    // wait for the animation to finish before attempting click
    await this.page
      .locator(".animation")
      .evaluate((el) => Promise.all(el.getAnimations().map((a) => a.finished)));
    await this.buttonRemoveOmission.click();
  }
}
