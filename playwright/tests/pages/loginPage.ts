import type { Locator, Page } from "@playwright/test";

export class loginPage {
  readonly headerLogin: Locator;
  readonly buttonLogin: Locator;

  constructor(protected readonly page: Page) {
    this.headerLogin = this.page.getByRole("heading", {
      name: "Kandidaatstellen",
    });
    this.buttonLogin = this.page.getByRole("button", {
      name: "Inloggen",
    });
  }
}
