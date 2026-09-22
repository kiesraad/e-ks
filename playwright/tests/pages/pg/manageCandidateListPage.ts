import type { Locator, Page } from "@playwright/test";
import type { Candidate } from "../../models/candidate";

export class ManageCandidateListPage {
  readonly buttonAddExistingCandidate: Locator;
  readonly buttonAddNewCandidate: Locator;
  readonly buttonSearchExistingCandidate: Locator;
  readonly textfieldInitials: Locator;
  readonly textfieldLastName: Locator;
  readonly textfieldFirstName: Locator;
  readonly textfieldLocality: Locator;
  readonly textfieldBSN: Locator;
  readonly textfieldBirthDay: Locator;
  readonly textfieldBirthMonth: Locator;
  readonly textfieldBirthYear: Locator;
  readonly dropdownGender: Locator;
  readonly buttonNext: Locator;
  readonly buttonSave: Locator;
  readonly textfieldPostalCode: Locator;
  readonly textfieldHouseNumber: Locator;
  readonly textfieldHouseNumberAddition: Locator;
  readonly textfieldStreetName: Locator;
  readonly buttonAdd: Locator;
  readonly buttonEditList: Locator;
  readonly buttonRemoveList: Locator;
  readonly buttonConfirmRemoveList: Locator;
  readonly buttonCSV: Locator;
  readonly headingCandidateList: Locator;
  readonly buttonRemoveCandidate: Locator;
  readonly buttonRemovefromApplication: Locator;
  readonly buttonRemovefromList: Locator;
  readonly buttonOverviewPage: Locator;

  constructor(protected readonly page: Page) {
    this.buttonAddExistingCandidate = this.page.getByRole("link", {
      name: "Bestaand",
    });
    this.buttonAddNewCandidate = this.page.getByRole("link", {
      name: "Nieuw",
    });
    this.buttonSearchExistingCandidate = this.page.getByLabel(
      "Zoek bestaande kandidaat",
    );
    this.textfieldInitials = this.page.getByLabel("Voorletters");
    this.textfieldLastName = this.page.getByLabel("Achternaam");
    this.textfieldFirstName = this.page.getByLabel("Roepnaam");
    this.textfieldLocality = this.page.getByLabel("Woonplaats");
    this.textfieldBSN = this.page.getByLabel("Burgerservicenummer (BSN)");
    this.textfieldBirthDay = this.page.getByRole("textbox", {
      name: "Geboortedatum",
    });
    this.textfieldBirthMonth = this.page.getByRole("textbox", {
      name: "Maand",
      exact: true,
    });
    this.textfieldBirthYear = this.page.getByRole("textbox", { name: "Jaar" });
    this.dropdownGender = this.page.getByLabel("Geslacht");
    this.buttonNext = this.page.getByRole("button", { name: "Volgende" });
    this.buttonSave = this.page.getByRole("button", { name: "Opslaan" });
    this.textfieldPostalCode = this.page.getByLabel("Postcode");
    this.textfieldHouseNumber = this.page.getByLabel("Huisnummer", {
      exact: true,
    });
    this.textfieldHouseNumberAddition = this.page.getByLabel(
      "Huisnummer toevoeging",
    );
    this.textfieldStreetName = this.page.getByLabel("Straatnaam");
    this.buttonAdd = this.page.getByRole("button", { name: "Toevoegen" });
    this.buttonEditList = this.page.getByRole("link", { name: "Aanpassen" });
    this.buttonRemoveList = this.page.getByRole("link", {
      name: "Kandidatenlijst verwijderen",
    });
    this.buttonConfirmRemoveList = this.page.getByRole("button", {
      name: "Kandidatenlijst verwijderen",
      exact: true,
    });
    this.buttonCSV = this.page.getByRole("link", {
      name: "Import en export kandidatenlijst",
    });
    this.headingCandidateList = this.page.getByRole("heading", {
      name: "Kandidatenlijst",
    });
    this.buttonRemoveCandidate = this.page.getByRole("link", {
      name: "Kandidaat verwijderen",
    });
    this.buttonRemovefromApplication = this.page.getByRole("button", {
      name: "Uit applicatie verwijderen",
    });
    this.buttonRemovefromList = this.page.getByRole("button", {
      name: "Uit deze lijst verwijderen",
    });
    this.buttonOverviewPage = this.page.getByRole("link", {
      name: "Verder naar het startscherm",
    });
  }

  async getCandidateLocator(candidateName: string) {
    return this.page.getByRole("cell", { name: candidateName });
  }

  async getDistrictLocator(districtName: string) {
    return this.page.getByRole("listitem", { name: districtName });
  }

  async addExistingCandidates(candidates: string[]) {
    for (const candidate of candidates) {
      await this.buttonAddExistingCandidate.click();

      // search first part of the name
      await this.buttonSearchExistingCandidate.pressSequentially(
        candidate.slice(0, 5),
      );

      await this.page
        .getByRole("row", { name: candidate })
        .getByRole("button")
        .click();

      await this.page.getByRole("link", { name: "Sluiten" }).first().click();
    }
  }

  async addNewCandidates(candidates: Candidate[]) {
    for (const candidate of candidates) {
      await this.buttonAddNewCandidate.click();
      await this.textfieldInitials.fill(candidate.initials);
      await this.textfieldLastName.fill(candidate.lastName);
      await this.textfieldFirstName.fill(candidate.firstName ?? "");
      // Step 1 "Woonplaats" is the place of residence (personal data).
      await this.textfieldLocality.fill(candidate.locality ?? "");
      await this.textfieldBSN.fill(candidate.bsn ?? "");
      await this.textfieldBirthDay.fill(candidate.dateOfBirth?.day ?? "");
      await this.textfieldBirthMonth.fill(candidate.dateOfBirth?.month ?? "");
      await this.textfieldBirthYear.fill(candidate.dateOfBirth?.year ?? "");
      if (candidate.gender) {
        await this.dropdownGender.selectOption(candidate.gender);
      }
      await this.buttonNext.click();
      await this.textfieldPostalCode.fill(candidate.postalCode ?? "");
      await this.textfieldHouseNumber.fill(candidate.houseNumber ?? "");
      await this.textfieldHouseNumberAddition.fill(
        candidate.houseNumberAddition ?? "",
      );
      await this.textfieldStreetName.fill(candidate.streetName ?? "");
      // Step 2 "Woonplaats" is the address locality (correspondence address).
      await this.textfieldLocality.fill(candidate.locality ?? "");
      await this.buttonAdd.click();
      await this.headingCandidateList.waitFor();
    }
  }

  async selectDistricts(districts: string[]) {
    for (const district of districts) {
      await this.page.getByRole("checkbox", { name: district }).check();
    }
    await this.buttonSave.click();
    await this.headingCandidateList.waitFor();
  }

  async removeDistricts(districts: string[]) {
    await this.buttonEditList.click();
    for (const district of districts) {
      await this.page.getByRole("checkbox", { name: district }).uncheck();
    }
    await this.buttonSave.click();
    await this.headingCandidateList.waitFor();
  }

  async addDistricts(districts: string[]) {
    await this.buttonEditList.click();
    for (const district of districts) {
      await this.page.getByRole("checkbox", { name: district }).check();
    }
    await this.buttonSave.click();
    await this.headingCandidateList.waitFor();
  }

  async removeList() {
    await this.buttonEditList.click();
    await this.buttonRemoveList.click();
    await Promise.all([
      this.page.waitForURL(/\/candidate-lists$/),
      this.buttonConfirmRemoveList.click(),
    ]);
  }

  async deleteCandidates(candidates: string[]) {
    for (const candidate of candidates) {
      await (await this.getCandidateLocator(candidate)).click();
      await this.buttonRemoveCandidate.click();
      await this.buttonRemovefromApplication.click();
    }
  }

  async deleteCandidatesFromList(candidates: string[]) {
    for (const candidate of candidates) {
      await (await this.getCandidateLocator(candidate)).click();
      await this.buttonRemoveCandidate.click();
      await this.buttonRemovefromList.click();
    }
  }
}
