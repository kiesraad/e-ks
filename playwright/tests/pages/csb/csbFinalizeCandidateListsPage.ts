import type { Locator, Page } from "@playwright/test";

export class CsbFinalizeCandidateListsPage {
  readonly linkHearingDetails: Locator;
  readonly headerFinalize: Locator;
  readonly downloadPdfDraft: Locator;
  readonly downloadWordDraft: Locator;
  readonly downloadFinalZip: Locator;
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
    this.downloadPdfDraft = this.page
      .getByRole("link", { name: "Download PDF" })
      .and(this.page.locator("#i4-draft-pdf"));
    this.downloadWordDraft = this.page
      .getByRole("link", { name: "Download Word" })
      .and(this.page.locator("#i4-draft-docx"));
    this.downloadFinalZip = this.page
      .getByRole("link", { name: "Download afrondings ZIP" })
      .and(this.page.locator("#final-zip"));
    this.buttonAddObjection = this.page.getByRole("link", {
      name: "Bezwaar toevoegen",
    });
    this.textfieldObjection = this.page.getByLabel("Bezwaar tekst");
    this.buttonSaveObjection = this.page.getByRole("button", {
      name: "Opslaan",
    });
    this.buttonDeleteObjection = this.page.getByRole("button", {
      name: "Verwijderen",
    });
  }

  async addObjection(objectionText: string) {
    await this.buttonAddObjection.click();
    await this.textfieldObjection.fill(objectionText);
    await this.buttonSaveObjection.click();
  }

  async deleteObjection(objectionText: string) {
    const objectionLink = this.page.getByRole("link", { name: objectionText });
    await objectionLink.click();
    await this.page
      .locator(".animation")
      .evaluate((el) => Promise.all(el.getAnimations().map((a) => a.finished)));
    await this.buttonDeleteObjection.click();
  }
}
