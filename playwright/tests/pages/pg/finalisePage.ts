import type { Locator, Page } from "@playwright/test";

export class FinalisePage {
  readonly linkDownloadNl: Locator;
  readonly linkDownloadFry: Locator;
  readonly linkRegisteredDesignation: Locator;
  readonly linkNoLegalName: Locator;
  readonly linkBSN: Locator;
  readonly linkDateOfBirth: Locator;
  readonly linkPlaceOfResidenceNotFound: Locator;
  readonly linkAdressIncomplete: Locator;
  readonly linkAdressNotFound: Locator;
  readonly linkTooYoung: Locator;
  readonly linkTooManyCandidates: Locator;
  readonly linkIncorrectDate: Locator;
  readonly linkCandidateList: Locator;

  constructor(protected readonly page: Page) {
    this.linkDownloadNl = this.page.getByRole("link", {
      name: "Alles in één zip",
    });
    this.linkDownloadFry = this.page.getByRole("link", {
      name: "Alles yn ien zip",
    });
    this.linkRegisteredDesignation = this.page.getByRole("link", {
      name: "Geregistreerde aanduiding",
    });
    this.linkNoLegalName = this.page.getByRole("link", {
      name: "1 statutaire naam te weinig",
    });
    this.linkBSN = this.page.getByRole("link", {
      name: "BSN",
    });
    this.linkDateOfBirth = this.page.getByRole("link", {
      name: "Geboortedatum",
      exact: true,
    });
    this.linkPlaceOfResidenceNotFound = this.page.getByRole("link", {
      name: "Woonplaats niet gevonden in de BAG",
    });
    this.linkAdressIncomplete = this.page.getByRole("link", {
      name: "Adres",
      exact: true,
    });
    this.linkAdressNotFound = this.page.getByRole("link", {
      name: "Adres niet gevonden in de BAG",
    });
    this.linkTooYoung = this.page.getByRole("link", {
      name: "Te jong",
    });
    this.linkTooManyCandidates = this.page.getByRole("link", {
      name: /\d+ (kandidaat|kandidaten) te veel/,
    });
    this.linkIncorrectDate = this.page.getByRole("link", {
      name: "Geboortedatum lijkt onjuist",
    });
    this.linkCandidateList = this.page.getByRole("link", {
      name: "Kandidatenlijst",
    });
  }
}
