import type { Locator, Page } from "@playwright/test";

export class PoliticalGroupPage {
  readonly headerGeneralInformation: Locator;
  readonly selectMoreThan16Seats: Locator;
  readonly selectLessThan16Seats: Locator;
  readonly selectNoSeats: Locator;
  readonly selectRegisteredPoliticalGroup: Locator;
  readonly textfieldRegisteredDesignation: Locator;
  readonly textfieldLegalName: Locator;
  readonly textfieldCombinedDesignation: Locator;
  readonly buttonSaveAndNext: Locator;
  readonly linkCandidateLists: Locator;

  constructor(protected readonly page: Page) {
    this.headerGeneralInformation = this.page.getByRole("heading", {
      name: "Basisgegevens",
    });
    this.buttonSaveAndNext = this.page.getByRole("button", {
      name: "Opslaan en volgende",
    });
    this.selectMoreThan16Seats = this.page.getByRole("radio", {
      name: "16 of meer zetels",
    });
    this.selectLessThan16Seats = this.page.getByRole("radio", {
      name: "1 tot 15 zetels",
    });
    this.selectNoSeats = this.page.getByRole("radio", {
      name: "0 zetels",
    });
    this.selectRegisteredPoliticalGroup = this.page.getByRole("combobox", {
      name: "Geregistreerde politieke groepering",
    });
    this.textfieldRegisteredDesignation = this.page.getByRole("textbox", {
      name: "Geregistreerde aanduiding",
    });
    this.textfieldLegalName = this.page.getByRole("textbox", {
      name: "Volledige statutaire naam",
    });
    this.textfieldCombinedDesignation = this.page.getByRole("textbox", {
      name: "Samengevoegde aanduiding",
    });
    this.linkCandidateLists = this.page.getByRole("link", {
      name: "Kandidatenlijsten",
    });
  }

  async open() {
    await this.page.goto("/political-group/information");
  }

  async save() {
    await this.page
      .getByRole("button", { name: "Opslaan en volgende" })
      .click();
  }

  /**
   * Selects the previous election result using the visible radio label.
   */
  async selectHasMoreThan16Seats(input: string) {
    await this.page.getByRole("radio", { name: input }).check();
  }

  /**
   * Picks a committee registration by its appellation, which fills and locks
   * the designation, legal name and previous election result.
   */
  async selectRegisteredPoliticalGroupNamed(appellation: string) {
    await this.selectRegisteredPoliticalGroup.selectOption({
      label: appellation,
    });
  }

  async setLegalName(legalName: string) {
    await this.textfieldLegalName.fill(legalName);
  }

  async setRegisteredDesignation(registeredDesignation: string) {
    await this.page
      .getByRole("textbox", { name: "Geregistreerde aanduiding" })
      .fill(registeredDesignation);
  }

  async setCombinedDesignation(combinedDesignation: string) {
    await this.page
      .getByRole("textbox", { name: "Samengevoegde aanduiding" })
      .fill(combinedDesignation);
  }
}
