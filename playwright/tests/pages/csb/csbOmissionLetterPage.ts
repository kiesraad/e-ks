import type { Locator, Page } from "@playwright/test";

export class CsbOmissionLetterPage {
  readonly buttonBack: Locator;
  readonly headerSubtext: Locator;
  readonly downloadPdf: Locator;
  readonly downloadWord: Locator;

  constructor(protected readonly page: Page) {
    this.buttonBack = this.page.getByRole("link", {
      name: "Terug",
    });
    this.headerSubtext = this.page.getByText(
      "In de verzuimbrief staan alle verzuimen die bij deze politieke groepering zijn vastgesteld.",
    );
    this.downloadPdf = this.page.getByRole("link", { name: "Download PDF" });
    this.downloadWord = this.page.getByRole("link", { name: "Download Word" });
  }
}
