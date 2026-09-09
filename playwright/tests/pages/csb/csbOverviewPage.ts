import type { Locator, Page } from "@playwright/test";

export class CsbOverviewPage {
  readonly linkAuditLog: Locator;
  readonly linkLogout: Locator;
  readonly buttonLanguageNL: Locator;
  readonly buttonLanguageEN: Locator;
  readonly headerElection: Locator;
  readonly linkExamination: Locator;
  readonly linkRegisteredPoliticalGroups: Locator;

  constructor(protected readonly page: Page) {
    this.linkAuditLog = this.page.getByRole("link", {
      name: "Logboek",
    });
    this.linkLogout = this.page.getByRole("link", {
      name: "Afmelden",
    });
    this.buttonLanguageNL = this.page.getByRole("button", {
      name: "NL",
    });
    this.buttonLanguageEN = this.page.getByRole("button", {
      name: "EN",
    });
    this.headerElection = this.page.getByRole("heading", {
      name: "Eerste Kamerverkiezing der Staten-Generaal 2027",
    });
    this.linkExamination = this.page.getByRole("link", {
      name: "Onderzoek",
    });
    // The phase 0 card; matched on its phase label so title and footer text
    // can change freely.
    this.linkRegisteredPoliticalGroups = this.page.getByRole("link", {
      name: "Fase 0",
    });
  }
}
