// Enhance country code inputs with flag icons and keyboard navigation.
import { isNetherlands, normaliseCountryCode } from "./country-code";

const COUNTRY_INPUT_SELECTOR = ".country-input";

// Places of residence in the Caribbean Netherlands: country code NL, but an
// authorised person is needed instead of a Dutch correspondence address.
// Mirrors CARIBBEAN_NL_PLACES in src/structs/common/place_of_residence.rs.
const CARIBBEAN_NL_PLACES = new Set([
  "kralendijk",
  "rincon",
  "bonaire",
  "saba",
  "sint eustatius",
]);

// Only the person/candidate personal-data form has this input; on other forms
// with a country input (e.g. list submitters) the selector matches nothing.
const PLACE_OF_RESIDENCE_SELECTOR = 'input[name="place_of_residence"]';

function getPlaceOfResidenceInput(): HTMLInputElement | null {
  return document.querySelector(PLACE_OF_RESIDENCE_SELECTOR);
}

type CountryInputElements = {
  textInput: HTMLInputElement;
  flagIcon: HTMLSpanElement;
  list: HTMLElement;
  items: HTMLLIElement[];
  nlIndex: number;
};

/**
 * Collects and validates the required DOM elements for a country input control.
 * Returns null when required structure is missing.
 */
function getCountryInputElements(input: Element): CountryInputElements | null {
  const textInput = input.querySelector("input");
  const flagIcon = input.querySelector(".icon");
  const list = input.querySelector("ul");
  const items = Array.from(list?.querySelectorAll("li") || []);
  const nlIndex = items.findIndex((item) => item.dataset.country === "NL");

  if (!textInput || !flagIcon || !list || items.length === 0) {
    console.error("Country input is missing required elements");
    return null;
  }

  if (nlIndex === -1) {
    console.error("Country input list is missing NL country code");
    return null;
  }

  return {
    textInput,
    flagIcon: flagIcon as HTMLSpanElement,
    list,
    items,
    nlIndex,
  };
}

/**
 * Applies input configuration to improve typing behavior.
 */
function configureTextInput(textInput: HTMLInputElement) {
  // disable built-in browser autocomplete
  textInput.autocomplete = "off";
  textInput.autocapitalize = "characters";
}

/**
 * Shows the suggestion list.
 */
function showList(list: HTMLElement) {
  list.style.display = "block";
}

/**
 * Hides the suggestion list.
 */
function hideList(list: HTMLElement) {
  list.style.display = "none";
}

/**
 * Toggles the hint visibility based on the selected country and, when present,
 * the place of residence. A place in the Caribbean Netherlands counts as
 * outside NL: an authorised person is needed instead of a Dutch
 * correspondence address.
 */
function updateVisibility(textInput: HTMLInputElement) {
  const place = getPlaceOfResidenceInput()?.value.trim().toLowerCase();
  const is_nl =
    isNetherlands(textInput) && !(place && CARIBBEAN_NL_PLACES.has(place));

  // toggle elements with class hide-nl
  document.querySelectorAll(".hide-nl").forEach((el) => {
    (el as HTMLElement).style.display = is_nl ? "none" : "";
  });

  // toggle elements with class show-nl
  document.querySelectorAll(".show-nl").forEach((el) => {
    (el as HTMLElement).style.display = is_nl ? "" : "none";
  });
}

/**
 * Updates the flag icon to match the current country code value.
 */
function setFlagIcon(
  textInput: HTMLInputElement,
  items: HTMLLIElement[],
  flagIcon: HTMLSpanElement,
) {
  const inputValue = textInput.value.toUpperCase();
  const match = items.find((item) => item.dataset.country === inputValue);
  const icon: HTMLElement | null | undefined = match?.querySelector(".icon");
  flagIcon.innerText = icon?.innerText || "🌐";
}

/**
 * Ensures a default country code is set when the input is empty.
 */
function selectDefaultCountry(textInput: HTMLInputElement) {
  if (textInput.value === "") {
    textInput.value = "NL";
  }
}

/**
 * Applies the active state to the selected list item and scrolls it into view.
 */
function setActiveIndex(items: HTMLLIElement[], index: number) {
  items.forEach((item, itemIndex) => {
    item.classList.toggle("active", itemIndex === index);
  });
  items[index]?.scrollIntoView({ block: "center" });
}

/**
 * Finds the next active index based on the current input value.
 * Falls back to the NL index if no match is found.
 */
function findActiveIndex(
  textInput: HTMLInputElement,
  items: HTMLLIElement[],
  nlIndex: number,
) {
  const inputValue = textInput.value.toUpperCase();
  const matchIndex = items.findIndex(
    (item) =>
      inputValue.length > 0 && item.dataset.country?.startsWith(inputValue),
  );
  return matchIndex === -1 ? nlIndex : matchIndex;
}

/**
 * Wires up all behaviors for a single country input instance.
 */
function initCountryInput(elements: CountryInputElements) {
  const { textInput, flagIcon, list, items, nlIndex } = elements;
  let active = 0;

  configureTextInput(textInput);
  selectDefaultCountry(textInput);

  const updateSuggestions = () => {
    showList(list);
    active = findActiveIndex(textInput, items, nlIndex);
    setActiveIndex(items, active);
    setFlagIcon(textInput, items, flagIcon);
    updateVisibility(textInput);
  };

  // render initial icon
  setFlagIcon(textInput, items, flagIcon);
  updateVisibility(textInput);

  // the place of residence also determines the visibility (Caribbean
  // Netherlands places require an authorised person while the country is NL)
  const placeInput = getPlaceOfResidenceInput();
  ["input", "change"].forEach((eventName) => {
    placeInput?.addEventListener(eventName, () => updateVisibility(textInput));
  });

  // show the suggestion list when focus on country code input
  textInput.addEventListener("focus", () => {
    textInput.select();
    updateSuggestions();
  });

  // Commit the typed value in its canonical form, so `nl` counts as NL for
  // the address lookup and locality suggestions listening for `change`.
  textInput.addEventListener("change", () => normaliseCountryCode(textInput));

  textInput.addEventListener("blur", () => {
    setTimeout(() => {
      hideList(list);
      updateVisibility(textInput);
    }, 200);
  });

  textInput.addEventListener("input", updateSuggestions);

  items.forEach((item) => {
    item.addEventListener("click", () => {
      textInput.value = item.dataset.country || "";
      setFlagIcon(textInput, items, flagIcon);
      updateVisibility(textInput);
      hideList(list);
      textInput.dispatchEvent(new Event("change", { bubbles: true }));
    });
  });

  textInput.addEventListener("keydown", (event) => {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      active = (active + 1) % items.length;
      setActiveIndex(items, active);
      return;
    }

    if (event.key === "ArrowUp") {
      event.preventDefault();
      active = (active - 1 + items.length) % items.length;
      setActiveIndex(items, active);
      return;
    }

    if (event.key === "Enter") {
      event.preventDefault();
      const selectedIndex = active % items.length;
      textInput.value = items[selectedIndex].dataset.country || "";
      setFlagIcon(textInput, items, flagIcon);
      updateVisibility(textInput);
      hideList(list);
      textInput.dispatchEvent(new Event("change", { bubbles: true }));
    }
  });
}

/**
 * Initializes all country input controls on the page.
 */
export default function countryCodeInput() {
  // Make flag icon match country code input
  const countryInputs = document.querySelectorAll(COUNTRY_INPUT_SELECTOR);

  countryInputs.forEach((input) => {
    const elements = getCountryInputElements(input);
    if (!elements) {
      return;
    }
    initCountryInput(elements);
  });
}
