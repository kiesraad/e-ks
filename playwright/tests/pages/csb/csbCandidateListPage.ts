import type { Locator, Page } from "@playwright/test";

export class CsbCandidateListPage {
  readonly headerCandidateList: Locator;
  readonly linkAddOmission: Locator;
  readonly linkManageOmissions: Locator;

  constructor(protected readonly page: Page) {
    this.headerCandidateList = this.page.getByRole("heading", {
      name: "Kandidatenlijst",
      exact: true,
    });
    this.linkAddOmission = this.page.getByRole("link", {
      name: "Verzuim toevoegen",
    });
    this.linkManageOmissions = this.page.getByRole("link", {
      name: "Overzicht",
    });
  }

  async getElectoralDistrict(page: Page, districts: string[]): Promise<string> {
    for (const district of districts) {
      // Match the numbered district tag ("1. Groningen") rather than the bare
      // name, which can also appear as a candidate's place of residence.
      if (await page.getByText(new RegExp(`\\d+\\. ${district}`)).isVisible()) {
        return district;
      }
    }
    throw new Error(
      `None of the districts [${districts.join(", ")}] were visible on the page`,
    );
  }
}
