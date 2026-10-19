// Use cases:
// CSB maakt een proces-verbaal van het onderzoek naar de kandidatenlijsten (model I 1) aan zonder verzuimen.
// CSB maakt een proces-verbaal van het onderzoek naar de kandidatenlijsten aan (model I1)
// CSB maakt een verzuimbrief per partij aan
// CSB stelt de definitieve kandidatatenlijsten vast in een proces-verbaal over geldigheid en nummering kandidatenlijsten (model I4)
// CSB maakt een concept proces-verbaal model I4 aan in e-KS

import { readFile, stat } from "node:fs/promises";
import { expect } from "@playwright/test";
import { test } from "./fixtures.ts";
import { CsbExaminationPage } from "./pages/csb/csbExaminationPage.ts";
import { CsbFinalizeCandidateListsPage } from "./pages/csb/csbFinalizeCandidateListsPage.ts";
import { CsbFinalizeExaminationPage } from "./pages/csb/csbFinalizeExaminationPage.ts";
import { CsbHearingDetailsPage } from "./pages/csb/csbHearingDetailsPage.ts";
import { CsbOmissionLetterPage } from "./pages/csb/csbOmissionLetterPage.ts";
import { CsbOmissionsDeclarationsOfSupportPage } from "./pages/csb/csbOmissionsDeclarationsOfSupport.ts";
import { CsbPoliticalGroupPage } from "./pages/csb/csbPoliticalGroupPage.ts";

test.describe("create documents", async () => {
  test("I 1", async ({ csbImport }) => {
    const { page } = csbImport;
    const finalizeExaminationPage = new CsbFinalizeExaminationPage(page);
    const hearingDetailsPage = new CsbHearingDetailsPage(page);
    const examinationPage = new CsbExaminationPage(page);

    await page.goto("/csb/examination");
    await examinationPage.buttonFinalise.click();
    await expect(finalizeExaminationPage.headerExamination).toBeVisible();

    // add hearing details
    await finalizeExaminationPage.linkHearingDetails.click();
    await expect(hearingDetailsPage.headerHearing).toBeVisible();
    await hearingDetailsPage.setDateOfHearing("30", "11", "2027");
    await hearingDetailsPage.textfieldTimeOfHearing.fill("10:57");
    await hearingDetailsPage.textfieldNameOfChairperson.fill("Vera van Veen");
    await page.locator("#signer_0").fill("Jan Jansen");
    await page.locator("#signer_1").fill("Diede de Bruin");
    await hearingDetailsPage.buttonSave.click();
    await expect(finalizeExaminationPage.headerExamination).toBeVisible();

    // download I 1 in pdf format
    const pdfPromise = page.waitForEvent("download");
    await finalizeExaminationPage.downloadI1pdf.click();
    const pdfDownload = await pdfPromise;
    expect(pdfDownload.suggestedFilename()).toMatch(/i1-proces-verbaal\.pdf/);
    expect((await stat(await pdfDownload.path())).size).toBeGreaterThan(1024);

    // download I 1 in word format
    const wordPromise = page.waitForEvent("download");
    await finalizeExaminationPage.downloadI1word.click();
    const wordDownload = await wordPromise;
    expect(wordDownload.suggestedFilename()).toMatch(/i1-proces-verbaal\.docx/);
    expect((await stat(await wordDownload.path())).size).toBeGreaterThan(1024);
  });

  test("Omission letters", async ({ csbImport }) => {
    const { page, groupName } = csbImport;
    const finalizeExaminationPage = new CsbFinalizeExaminationPage(page);
    const examinationPage = new CsbExaminationPage(page);
    const politicalGroupPage = new CsbPoliticalGroupPage(page);
    const omissionPage = new CsbOmissionsDeclarationsOfSupportPage(page);
    const omissionLetterPage = new CsbOmissionLetterPage(page);
    const expectedName = groupName.toLowerCase().replace(/\s+/g, "-");

    await page.goto("/csb/examination");

    // add omission and finalize investigation
    await examinationPage.selectPoliticalGroup(groupName);
    await politicalGroupPage.selectedGroup(groupName);
    await politicalGroupPage.linkSupportDeclarations.click();
    await omissionPage.checkboxAllLists.check();
    await omissionPage.buttonMissingAllDistricts.click();
    await omissionPage.buttonAddAndClose.click();
    await expect(
      page.getByText(
        "Voor alle kieskringen ontbreken ondersteuningsverklaringen",
      ),
    ).toBeVisible();
    await politicalGroupPage.switchFinalize.click();
    await politicalGroupPage.buttonBack.click();

    // go to finalize examination page and verify that the omission is added for this group
    await examinationPage.buttonFinalise.click();
    await expect(finalizeExaminationPage.headerExamination).toBeVisible();
    await finalizeExaminationPage.selectPoliticalGroup(groupName);
    await expect(
      page.getByText(
        "Voor alle kieskringen ontbreken ondersteuningsverklaringen",
      ),
    ).toBeVisible();

    // download omission letter in pdf format
    const pdfPromise = page.waitForEvent("download");
    await omissionLetterPage.downloadPdf.click();
    const pdfDownload = await pdfPromise;
    expect(pdfDownload.suggestedFilename()).toBe(
      `verzuimbrief-${expectedName}-ek27.pdf`,
    );
    expect((await stat(await pdfDownload.path())).size).toBeGreaterThan(1024);

    // download omission letter in word format
    const wordPromise = page.waitForEvent("download");
    await omissionLetterPage.downloadWord.click();
    const wordDownload = await wordPromise;
    expect(wordDownload.suggestedFilename()).toBe(
      `verzuimbrief-${expectedName}-ek27.docx`,
    );
    expect((await stat(await wordDownload.path())).size).toBeGreaterThan(1024);

    // download omission letters in zip format
    await omissionLetterPage.buttonBack.click();
    const zipPromise = page.waitForEvent("download");
    await finalizeExaminationPage.downloadOmissionLetters.click();
    const zipDownload = await zipPromise;
    expect(zipDownload.suggestedFilename()).toMatch(/verzuimbrieven-ek27\.zip/);
    expect((await stat(await zipDownload.path())).size).toBeGreaterThan(1024);
  });

  test("I 4", async ({ csbImport }) => {
    const { page } = csbImport;
    const finalizeCandidateListsPage = new CsbFinalizeCandidateListsPage(page);
    const hearingDetailsPage = new CsbHearingDetailsPage(page);

    await page.goto("/csb/finalise");

    // add hearing details
    await finalizeCandidateListsPage.linkHearingDetails.click();
    await expect(hearingDetailsPage.headerHearing).toBeVisible();
    await hearingDetailsPage.setDateOfHearing("22", "03", "2027");
    await hearingDetailsPage.textfieldTimeOfHearing.fill("09:17");
    await hearingDetailsPage.textfieldNameOfChairperson.fill("Jan Jansen");
    await page.locator("#signer_0").fill("Vera van Veen");
    await page.locator("#signer_1").fill("Diede de Bruin");
    await hearingDetailsPage.buttonSave.click();
    await expect(finalizeCandidateListsPage.headerFinalize).toBeVisible();

    // add objections, then delete one and verify that the other is still present;
    // objections are shared by all browsers running this test, so make them unique
    const suffix = Math.random().toString(36).slice(2, 10);
    const firstObjection = `Eerste testbezwaar ${suffix}`;
    const secondObjection = `Tweede testbezwaar ${suffix}`;
    await finalizeCandidateListsPage.addObjection(firstObjection);
    await expect(page.getByText(firstObjection)).toBeVisible();
    await finalizeCandidateListsPage.addObjection(secondObjection);
    await expect(page.getByText(secondObjection)).toBeVisible();
    await finalizeCandidateListsPage.deleteObjection(firstObjection);
    await expect(page.getByText(firstObjection)).not.toBeVisible();
    await expect(page.getByText(secondObjection)).toBeVisible();

    // download draft I 4 in pdf format
    const pdfPromise = page.waitForEvent("download");
    await finalizeCandidateListsPage.downloadPdfDraft.click();
    const pdfDownload = await pdfPromise;
    expect(pdfDownload.suggestedFilename()).toBe(
      "Model_I4_Proces_Verbaal_Concept.pdf",
    );
    expect((await stat(await pdfDownload.path())).size).toBeGreaterThan(1024);

    // download draft I 4 in word format
    const wordPromise = page.waitForEvent("download");
    await finalizeCandidateListsPage.downloadWordDraft.click();
    const wordDownload = await wordPromise;
    expect(wordDownload.suggestedFilename()).toBe(
      "Model_I4_Proces_Verbaal_Concept.docx",
    );
    expect((await stat(await wordDownload.path())).size).toBeGreaterThan(1024);

    // download the final zip with the final I 4, the EMLs and the
    // election definition; entry names are stored uncompressed
    const zipPromise = page.waitForEvent("download");
    await finalizeCandidateListsPage.downloadFinalZip.click();
    const zipDownload = await zipPromise;
    expect(zipDownload.suggestedFilename()).toBe(
      "Afronding_Kandidaatstellen_ek27.zip",
    );
    const zip = (await readFile(await zipDownload.path())).toString("latin1");
    for (const entry of [
      "Model_I4_Proces_Verbaal.pdf",
      "Model_I4_Proces_Verbaal.docx",
      "Totaallijsten_",
      "Verkiezingsdefinitie_",
    ]) {
      expect(zip).toContain(entry);
    }

    // delete objection for clean up
    await finalizeCandidateListsPage.deleteObjection(secondObjection);
  });
});
