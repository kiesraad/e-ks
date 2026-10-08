// Auto-fill address fields using postal code and house number lookups.
import { isNetherlands } from "./country-code";

export default function addressLookup() {
  const postalCodeInput = document.getElementById(
    "postal_code",
  ) as HTMLInputElement | null;
  const houseNumberInput = document.getElementById(
    "house_number",
  ) as HTMLInputElement | null;
  const localityInput = document.getElementById(
    "locality",
  ) as HTMLInputElement | null;
  const streetNameInput = document.getElementById(
    "street_name",
  ) as HTMLInputElement | null;
  // country code is only included in some forms
  const countryCodeInput = document.getElementById(
    "country",
  ) as HTMLInputElement | null;

  const addressWarning = document.getElementById(
    "unknown-address",
  ) as HTMLElement | null;

  // only run if all form fields are found
  if (
    !postalCodeInput ||
    !houseNumberInput ||
    !localityInput ||
    !streetNameInput
  ) {
    return;
  }

  // `warning` is only meaningful on the lookup-driving inputs; locality and
  // street are user-editable when no match is found, so they never get warned.
  const allInputs = [
    postalCodeInput,
    houseNumberInput,
    localityInput,
    streetNameInput,
  ];
  const lookupInputs = [postalCodeInput, houseNumberInput];

  const setHighlight = (
    inputs: HTMLInputElement[],
    className: string,
    on: boolean,
  ) => {
    for (const input of inputs) {
      input.closest(".form-field")?.classList.toggle(className, on);
    }
  };

  function resetHighlights() {
    setHighlight(allInputs, "success", false);
    setHighlight(allInputs, "warning", false);

    if (addressWarning) {
      addressWarning.classList.add("hidden");
    }
  }

  const lookup = async () => {
    resetHighlights();

    postalCodeInput.value = postalCodeInput.value
      .replaceAll(/\s/g, "")
      .toUpperCase();

    // only perform lookup when postal code and house number are filled
    if (!postalCodeInput.value || !houseNumberInput.value) {
      return;
    }
    // skip lookup if country code input is present and not NL
    if (!isNetherlands(countryCodeInput)) {
      return;
    }

    // fetch address data from backend
    const url = `/lookup?pc=${encodeURIComponent(postalCodeInput.value)}&n=${encodeURIComponent(houseNumberInput.value)}`;
    const response = await fetch(url, {
      method: "GET",
      headers: {
        Accept: "application/json",
      },
    });

    const data = response.ok ? await response.json() : null;
    if (data?.wp && data?.pr) {
      localityInput.value = data.wp;
      streetNameInput.value = data.pr;

      // highlight the address fields to indicate they were auto-filled
      // (the warning was already hidden by resetHighlights above)
      setHighlight(allInputs, "success", true);
    } else {
      // No usable match — flag the inputs that drive the lookup so the user
      // knows the combination is unrecognised, and surface the warning message.
      setHighlight(lookupInputs, "warning", true);

      if (addressWarning) {
        addressWarning.classList.remove("hidden");
      }
    }
  };

  postalCodeInput.addEventListener("change", lookup);
  houseNumberInput.addEventListener("change", lookup);
  localityInput.addEventListener("change", resetHighlights);
  streetNameInput.addEventListener("change", resetHighlights);
  if (countryCodeInput != null) {
    countryCodeInput.addEventListener("change", lookup);
  }
}
