import type { Locator, Page } from "@playwright/test";

export class CsbFinalizeCandidateListsPage   {
  readonly linkHearingDetails: Locator;
  readonly headerFinalize: Locator;
  readonly downloadPdf: Locator;
  readonly downloadWord: Locator;
  readonly downloadEml: Locator;
  readonly buttonAddObjection: Locator;
  readonly textfieldObjection: Locator;
  readonly buttonSaveObjection: Locator;
  readonly buttonDeleteObjection: Locator;

  constructor(protected readonly page: Page) {
    this.linkHearingDetails = this.page.getByRole("link", {
      name: "Zitting details",
    });
    this.headerFinalize = this.page.getByRole("heading", {
      name: "Vaststellen kandidatenlijsten",
      exact: true,
    });
    this.downloadPdf = this.page.getByRole("link", { name: "Download PDF" });
    this.downloadWord = this.page.getByRole("link", { name: "Download Word" });
    this.downloadEml = this.page.getByRole("link", { name: "EML230b downloaden" });
    this.buttonAddObjection = this.page.getByRole("link", { name: "Bezwaar toevoegen" });
    this.textfieldObjection = this.page.getByLabel("Bezwaar tekst");
    this.buttonSaveObjection = this.page.getByRole("button", { name: "Opslaan" });
    this.buttonDeleteObjection = this.page.getByRole("button", { name: "Verwijderen" });
  }

async addObjection(objectionText: string) {
  await this.buttonAddObjection.click();
  await this.textfieldObjection.fill(objectionText);
  await this.buttonSaveObjection.click();
  }

async deleteObjection(objectionText: string) {
  const objectionLink = this.page.getByRole("link", { name: objectionText });
  await objectionLink.click();
  await this.buttonDeleteObjection.click();
}
}
