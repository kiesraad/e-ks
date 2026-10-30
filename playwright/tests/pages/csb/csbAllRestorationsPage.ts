import type { Locator, Page } from "@playwright/test";

export class CsbAllRestorationsPage {
  readonly headerAllRestorations: Locator;
  readonly buttonBack: Locator;
  readonly buttonAllRestorations: Locator;

  constructor(protected readonly page: Page) {
    this.headerAllRestorations = this.page.getByRole("heading", {
      level: 1,
      name: "Verzuimen en correcties",
    });
    this.buttonBack = this.page
      .locator(".header-content")
      .getByRole("link", { name: "Terug" });
    this.buttonAllRestorations = this.page
      .locator(".header-buttons")
      .getByRole("link", {
        name: "Verzuimen en correcties",
      });
  }
}
