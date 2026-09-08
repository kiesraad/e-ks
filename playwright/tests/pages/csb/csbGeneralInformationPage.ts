import type { Locator, Page } from "@playwright/test";

export class CsbGeneralInformationPage {
  readonly headerGeneralInformation: Locator;
  readonly linkAddAppellationOmission: Locator;
  readonly linkManageAppellationOmissions: Locator;
  readonly linkRegisteredDesignationStandalone: Locator;
  readonly textCorrectedNameStandalone: Locator;
  readonly textCorrectedNameCombined: Locator;
  readonly textCorrectedType: Locator;
  readonly linkBack: Locator;

  constructor(readonly page: Page) {
    this.headerGeneralInformation = this.page.getByRole("heading", {
      name: "Basisgegevens",
      exact: true,
    });
    this.linkAddAppellationOmission = this.page
      .getByRole("link", {
        name: "Verzuim toevoegen",
      })
      .nth(1);
    this.linkManageAppellationOmissions = this.page
      .getByRole("link", {
        name: "Overzicht",
      })
      .nth(1);
    this.linkRegisteredDesignationStandalone = this.page.getByRole("cell", {
      name: "Geregistreerde aanduiding",
    });
    this.textCorrectedNameStandalone = this.page
      .getByRole("row", { name: "Geregistreerde aanduiding:" })
      .getByRole("strong");
    this.textCorrectedNameCombined = this.page
      .getByRole("row", { name: "Samengevoegde aanduiding:" })
      .getByRole("strong");
    this.textCorrectedType = this.page
      .getByRole("row", { name: "Type lijstaanduiding:" })
      .getByRole("strong");
    this.linkBack = this.page.getByRole("link", { name: "Terug" });
  }
}
