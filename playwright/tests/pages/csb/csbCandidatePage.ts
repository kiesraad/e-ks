import type { Locator, Page } from "@playwright/test";

export class CsbCandidatePage {
  readonly linkAddOmission: Locator;
  readonly linkManageOmissions: Locator;
  readonly linkInitials: Locator;
  readonly linkLastNamePrefix: Locator;
  readonly linkLastName: Locator;
  readonly linkDateOfBirth: Locator;
  readonly linkPlaceOfResidence: Locator;
  readonly textCorrectedInitials: Locator;
  readonly textCorrectedLastNamePrefix: Locator;
  readonly textCorrectedLastName: Locator;
  readonly textCorrectedDateOfBirth: Locator;
  readonly textCorrectedPlaceOfResidence: Locator;

  constructor(protected readonly page: Page) {
    this.linkAddOmission = this.page.getByRole("link", {
      name: "Verzuim toevoegen",
    });
    this.linkManageOmissions = this.page.getByRole("link", {
      name: "Overzicht",
    });
    this.linkInitials = this.page.getByRole("cell", {
      name: "Voorletters",
    });
    this.linkLastNamePrefix = this.page.getByRole("cell", {
      name: "Voorvoegsel",
    });
    this.linkLastName = this.page.getByRole("cell", {
      name: "Achternaam",
    });
    this.linkDateOfBirth = this.page.getByRole("cell", {
      name: "Geboortedatum",
    });
    this.linkPlaceOfResidence = this.page.getByRole("cell", {
      name: "Woonplaats",
    });
    this.textCorrectedInitials = this.page
      .getByRole("row", { name: "Voorletters:" })
      .getByRole("strong");
    this.textCorrectedLastNamePrefix = this.page
      .getByRole("row", { name: "Voorvoegsel:" })
      .getByRole("strong");
    this.textCorrectedLastName = this.page
      .getByRole("row", { name: "Achternaam:" })
      .getByRole("strong");
    this.textCorrectedDateOfBirth = this.page
      .getByRole("row", { name: "Geboortedatum:" })
      .getByRole("strong");
    this.textCorrectedPlaceOfResidence = this.page
      .getByRole("row", { name: "Woonplaats:" })
      .getByRole("strong");
  }

  async getElectoralDistrict(page: Page, districts: string[]): Promise<string> {
    for (const district of districts) {
      if (await page.getByText(district).isVisible()) {
        return district;
      }
    }
    throw new Error(
      `None of the districts [${districts.join(", ")}] were visible on the page`,
    );
  }
}
