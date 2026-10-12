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
    const headerButtons = this.page.locator(".header-buttons");
    this.buttonBack = headerButtons.getByRole("link", { name: "Terug" });
    this.buttonRecoverOmissions = headerButtons.getByRole("link", {
      name: "Verzuimen herstellen",
    });
  }
}
