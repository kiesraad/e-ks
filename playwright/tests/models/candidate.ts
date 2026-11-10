import type { Representative } from "./representative";

export interface DateOfBirth {
  day?: string;
  month?: string;
  year?: string;
}

export interface Candidate {
  initials: string;
  lastNamePrefix?: string;
  lastName: string;
  firstName?: string;
  bsn?: string;
  gender?: string;
  dateOfBirth?: DateOfBirth;
  postalCode?: string;
  houseNumber?: string;
  houseNumberAddition?: string;
  streetName?: string;
  locality?: string;
  countryCode?: string;
  representative?: Representative;
}
