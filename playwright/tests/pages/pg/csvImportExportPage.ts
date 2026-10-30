import path from "node:path";
import type { Locator, Page } from "@playwright/test";

export class CsvImportExportPage {
  readonly buttonDownload: Locator;
  readonly buttonUpload: Locator;
  readonly buttonDownloadTemplate: Locator;
  readonly buttonContinue: Locator;
  readonly buttonCancel: Locator;
  readonly buttonClose: Locator;
  readonly headerImport: Locator;
  readonly textFailure: Locator;

  constructor(protected readonly page: Page) {
    this.buttonDownload = this.page.getByRole("link", {
      name: "Download kandidatenlijst",
    });
    // The file input's aria-label also has button role, so match the <button>.
    this.buttonUpload = this.page
      .getByRole("button", { name: "Upload kandidatenlijst" })
      .and(this.page.locator("button"));
    this.buttonDownloadTemplate = this.page.getByRole("link", {
      name: "Download sjabloon",
    });
    this.buttonContinue = this.page.getByRole("button", { name: "Doorgaan" });
    this.buttonCancel = this.page.getByRole("button", { name: "Annuleren" });
    this.buttonClose = this.page.getByRole("link", { name: "Sluiten" });
    this.headerImport = this.page.getByRole("heading", {
      name: "Import en export kandidatenlijst",
    });
    this.textFailure = this.page.getByText("Importeren niet gelukt");
  }

  async getValidationErrors(validationError: string) {
    return this.page.getByText(`Controleer veld ${validationError}`);
  }

  async uploadCsvFile(filePath: string) {
    const fileChooserPromise = this.page.waitForEvent("filechooser");

    await this.buttonUpload.click();

    // Depending on the flow a confirmation dialog ("Doorgaan") may appear
    // before the file chooser opens. Wait briefly for it and click it if it
    // shows; otherwise the chooser was opened directly by the upload click.
    await this.buttonContinue
      .waitFor({ state: "visible", timeout: 2000 })
      .then(() => this.buttonContinue.click())
      .catch(() => {});

    const fileChooser = await fileChooserPromise;
    await fileChooser.setFiles(
      path.join(__dirname, "../../testdata", filePath),
    );
  }
}
