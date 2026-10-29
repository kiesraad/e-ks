// Use cases:
// De lijstinleveraar vult de benodigde personalia (personalia kandidaat H1) in
// Er ontbreken kandidaten, de lijstinleveraar vult de missende kandidaten in

import { expect } from "@playwright/test";
import { test } from "./fixtures.ts";
import type { AuthorisedPerson } from "./models/authorisedPerson";
import type { Candidate } from "./models/candidate";
import { AuthorisedPersonPage } from "./pages/pg/authorisedPersonPage";
import { CandidateListsOverviewPage } from "./pages/pg/candidateListsOverviewPage";
import { CorrespondenceAddressPage } from "./pages/pg/correspondenceAddressPage";
import { CreatePersonPage } from "./pages/pg/createPersonPage";
import { EditListDetailsPage } from "./pages/pg/editListDetailsPage.ts";
import { ManageCandidateListPage } from "./pages/pg/manageCandidateListPage.ts";
import { randomName } from "./utils/random";

test.describe("create new person", async () => {
  const candidateAllFields: Candidate = {
    initials: "H",
    lastName: `Jansen ${randomName()}`,
    lastNamePrefix: "van",
    firstName: "Henk",
    gender: "male",
    dateOfBirth: { day: "12", month: "08", year: "1977" },
    postalCode: "6512EX",
    houseNumber: "26",
    streetName: "Castellastraat",
    locality: "Nijmegen",
  };

  const candidateMinimalFields: Candidate = {
    initials: "D",
    lastName: `Duif ${randomName()}`,
  };
  const candidates = [
    { candidate: candidateAllFields, description: "with all fields" },
    {
      candidate: candidateMinimalFields,
      description: "with minimal required fields",
    },
  ];
  for (const { candidate, description } of candidates) {
    test(description, async ({ login: page }) => {
      await page.goto("/candidate-lists");
      await new CandidateListsOverviewPage(page).buttonAddList.click();
      await new EditListDetailsPage(page).addDistricts(["Groningen"]);
      const manageCandidateListPage = new ManageCandidateListPage(page);
      await manageCandidateListPage.buttonAddNewCandidate.click();
      await new CreatePersonPage(page).setPersonalDetails(candidate);
      await new CorrespondenceAddressPage(page).setCorrespondenceAddress(
        candidate,
      );
      await expect(
        await manageCandidateListPage.getCandidateLocator(candidate.lastName),
      ).toBeVisible();
    });
  }

  test("living outside NL requires authorised person", async ({
    login: page,
  }) => {
    await page.goto("/candidate-lists");
    await new CandidateListsOverviewPage(page).buttonAddList.click();
    await new EditListDetailsPage(page).addDistricts(["Groningen"]);
    const manageCandidateListPage = new ManageCandidateListPage(page);
    await manageCandidateListPage.buttonAddNewCandidate.click();

    const authorisedPerson: AuthorisedPerson = {
      initials: "C",
      lastName: "Winter",
    };
    const candidate: Candidate = {
      initials: "H",
      lastName: `Jansen ${randomName()}`,
      countryCode: "VA",
      authorisedPerson: authorisedPerson,
    };

    await new CreatePersonPage(page).setPersonalDetails(candidate);
    await new AuthorisedPersonPage(page).setAuthorisedPerson(
      // biome-ignore lint/style/noNonNullAssertion: the test should fail if the authorised person is not set
      candidate.authorisedPerson!,
    );
    await expect(
      await manageCandidateListPage.getCandidateLocator(candidate.lastName),
    ).toBeVisible();
  });
});
