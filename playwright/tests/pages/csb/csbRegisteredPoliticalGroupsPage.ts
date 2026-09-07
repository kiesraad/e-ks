import type { Locator, Page } from "@playwright/test";

export class CsbRegisteredPoliticalGroupsPage {
  readonly header: Locator;
  readonly textNoGroups: Locator;
  readonly linkAdd: Locator;
  readonly headerAdd: Locator;
  readonly headerEdit: Locator;
  readonly textfieldAppellation: Locator;
  readonly textfieldVotes: Locator;
  readonly textfieldSeats: Locator;
  readonly buttonSave: Locator;
  readonly linkDelete: Locator;
  readonly buttonDeleteConfirm: Locator;

  constructor(protected readonly page: Page) {
    this.header = this.page.getByRole("heading", {
      name: "Geregistreerde politieke groeperingen",
    });
    this.textNoGroups = this.page.getByText(
      "Er zijn nog geen politieke groeperingen geregistreerd.",
    );
    this.linkAdd = this.page.getByRole("link", {
      name: "Politieke groepering toevoegen",
    });
    this.headerAdd = this.page.getByRole("heading", {
      name: "Politieke groepering toevoegen",
    });
    this.headerEdit = this.page.getByRole("heading", {
      name: "Politieke groepering bewerken",
    });
    this.textfieldAppellation = this.page.getByLabel(
      "Geregistreerde aanduiding",
    );
    this.textfieldVotes = this.page.getByLabel(
      "Stemmen bij de laatstgehouden verkiezing",
    );
    this.textfieldSeats = this.page.getByLabel(
      "Zetels bij de laatstgehouden verkiezing",
    );
    this.buttonSave = this.page.getByRole("button", { name: "Opslaan" });
    this.linkDelete = this.page.getByRole("link", { name: "Verwijderen" });
    this.buttonDeleteConfirm = this.page.getByRole("button", {
      name: "Verwijderen",
    });
  }

  /// The table row of the group with `appellation`.
  row(appellation: string): Locator {
    return this.page.getByRole("row").filter({
      has: this.page.getByRole("cell", { name: appellation, exact: true }),
    });
  }

  async fillForm(appellation: string, votes: string, seats: string) {
    await this.textfieldAppellation.fill(appellation);
    await this.textfieldVotes.fill(votes);
    await this.textfieldSeats.fill(seats);
  }

  /// Saves the dialog and waits for the redirect back to the list.
  async save() {
    await Promise.all([
      this.page.waitForURL(/\/csb\/registered-political-groups\?/),
      this.buttonSave.click(),
    ]);
  }
}
