import type { Locator, Page } from "@playwright/test";

export class CsbRecoveryOmissionsPage {
  readonly headerRecoveryOmissions: Locator;
  readonly buttonBack: Locator;
  readonly buttonRecoverOmissions: Locator;

  constructor(protected readonly page: Page) {
    this.headerRecoveryOmissions = this.page.getByRole("heading", {
      level: 1,
      name: "Verzuimen herstellen",
    });
    this.buttonBack = this.page
      .locator(".header-content")
      .getByRole("link", { name: "Terug" });
    this.buttonRecoverOmissions = this.page
      .locator(".header-buttons")
      .getByRole("link", {
        name: "Verzuimen herstellen",
      });
  }
}
