// The country code input is uppercased by CSS only, so its value can still be
// typed in lowercase. Every check against it goes through here.

/** Value as the backend sees it: trimmed, uppercased. */
export function normaliseCountryCode(input: HTMLInputElement): string {
  const value = input.value.trim().toUpperCase();
  if (input.value !== value) {
    input.value = value;
  }
  return value;
}

/**
 * Whether the form's country is the Netherlands. Forms without a country
 * input are Dutch-only.
 */
export function isNetherlands(input: HTMLInputElement | null): boolean {
  return input == null || input.value.trim().toUpperCase() === "NL";
}
