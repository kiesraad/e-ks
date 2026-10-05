 import { expect, type Locator, type Page } from "@playwright/test";
 
 export class CsbRecoveryGroupPage {
   readonly buttonBack: Locator;
   readonly buttonRecoverOmissions: Locator;
 
   constructor(protected readonly page: Page) {
     const headerButtons = this.page.locator(".header-buttons");
     this.buttonBack = headerButtons.getByRole("link", { name: "Terug" });
     this.buttonRecoverOmissions = headerButtons.getByRole("link", {
       name: "Verzuimen herstellen",
     });
   }

    async selectedGroup(politicalgroup: string) {
       await expect(
         this.page.getByRole("heading", { name: politicalgroup }),
       ).toBeVisible();
     }

  async recoverOmission(district: string, recovered: boolean) {
  const buttonName = recovered ? 'Hersteld' : 'Niet hersteld';
  await this.page
    .getByRole('row')
    .filter({ hasText: district })
    .getByRole('button', { name: buttonName, exact: true })
    .click();
}
  
 }
 
 
