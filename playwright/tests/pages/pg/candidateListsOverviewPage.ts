import type { Locator, Page } from "@playwright/test";

export class CandidateListsOverviewPage {
  readonly buttonAddList: Locator;
  readonly linkCandidateList: Locator;
  readonly headingAllCandidates: Locator;
  readonly linkFinalize: Locator;

  constructor(protected readonly page: Page) {
    // a link that opens the district form, or a submit button on
    // single-district elections where the one list is created directly
    const addListName = /(Kandidatenlijst|Nog een lijst) toevoegen/;
    this.buttonAddList = this.page
      .getByRole("link", { name: addListName })
      .or(this.page.getByRole("button", { name: addListName }));
    this.linkCandidateList = this.page.getByRole("link", {
      name: "Kandidatenlijst beheren",
    });
    this.headingAllCandidates = this.page.getByRole("heading", {
      name: "Adresboek",
    });
    this.linkFinalize = this.page.getByRole("link", {
      name: "Afronden",
      exact: true,
    });
  }

  async getDistrictLocator(districtName: string) {
    return this.page.getByRole("listitem", { name: districtName });
  }
}
