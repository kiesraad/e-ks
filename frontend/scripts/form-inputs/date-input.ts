// Assist date-of-birth inputs with numeric formatting and dash handling.
const DATE_INPUT_ROW_SELECTOR = 'span[class="date-input-row"]';

const DATE_INPUT_SELECTOR = 'input[class="date"]';

const DATE_OF_BIRTH_SEPARATE_SELECTOR = ".date-separate";
const DAY_INPUT_SELECTOR = 'input[class="day"]';
const MONTH_INPUT_SELECTOR = 'input[class="month"]';
const YEAR_INPUT_SELECTOR = 'input[class="year"]';

const DAY_DIGITS = 2;
const MONTH_DIGITS = 2;
const YEAR_DIGITS = 4;

const IGNORE_TAB_DURATION_MS = 400;

type Inputs = {
  separateInputs: HTMLSpanElement;
  dayInput: HTMLInputElement;
  monthInput: HTMLInputElement;
  yearInput: HTMLInputElement;
  actual: HTMLInputElement;
};

// Collect all date of birth inputs on the page.
function getDateInputs(): Inputs[] {
  const rows = document.querySelectorAll(DATE_INPUT_ROW_SELECTOR);
  return [...rows].map((row) => ({
    separateInputs: row.querySelector(
      DATE_OF_BIRTH_SEPARATE_SELECTOR,
    ) as HTMLSpanElement,
    dayInput: row.querySelector(DAY_INPUT_SELECTOR) as HTMLInputElement,
    monthInput: row.querySelector(MONTH_INPUT_SELECTOR) as HTMLInputElement,
    yearInput: row.querySelector(YEAR_INPUT_SELECTOR) as HTMLInputElement,
    actual: row.querySelector(DATE_INPUT_SELECTOR) as HTMLInputElement,
  }));
}

function sanitize(input: HTMLInputElement) {
  input.value = input.value.replaceAll(/[^\d]/g, "");
}

function handleDayInput(
  dayInput: HTMLInputElement,
  month_input: HTMLInputElement,
): boolean {
  sanitize(dayInput);
  if (dayInput.value > "31") {
    dayInput.value = `0${dayInput.value}`;
  }
  dayInput.value = dayInput.value.slice(0, DAY_DIGITS);
  if (dayInput.value.length === DAY_DIGITS) {
    month_input.select();
    return true;
  }
  return false;
}

function handleMonthInput(
  monthInput: HTMLInputElement,
  year_input: HTMLInputElement,
): boolean {
  sanitize(monthInput);
  if (monthInput.value > "12") {
    monthInput.value = `0${monthInput.value}`;
  }
  monthInput.value = monthInput.value.slice(0, MONTH_DIGITS);
  if (monthInput.value.length === MONTH_DIGITS) {
    year_input.select();
    return true;
  }
  return false;
}

function handleYearInput(yearInput: HTMLInputElement) {
  sanitize(yearInput);
  if (yearInput.value.length > 0 && yearInput.value[0] > "2") {
    yearInput.value = `19${yearInput.value}`;
  }
  yearInput.value = yearInput.value.slice(0, YEAR_DIGITS);
}

function formatSingleDigit(input: HTMLInputElement) {
  if (input.value.length !== 1) {
    return;
  }
  input.value = `0${input.value}`;
}

function updateActual(input: Inputs) {
  if (
    input.dayInput.value.length === 0 &&
    input.monthInput.value.length === 0 &&
    input.yearInput.value.length === 0
  ) {
    input.actual.value = "";
  } else {
    input.actual.value = `${input.dayInput.value}-${input.monthInput.value}-${input.yearInput.value}`;
  }
}

function updateVisible(inputs: Inputs) {
  // swap inputs for JavaScript enjoyers
  inputs.actual.type = "hidden";
  inputs.separateInputs.classList.remove("hidden");

  if (inputs.actual.value === "") {
    return;
  }

  const parts = inputs.actual.value.split("-");
  inputs.dayInput.value = parts[0];
  inputs.monthInput.value = parts[1];
  inputs.yearInput.value = parts[2];
}

function tabAdvanceCheck(
  event: KeyboardEvent,
  ignoreNextTab: boolean,
): boolean {
  if (event.key === "Tab" && ignoreNextTab) {
    event.preventDefault();
    return false;
  }
  return ignoreNextTab;
}

// Enforce date format DD-MM-YYYY for date_of_birth inputs.
export default function dateInput() {
  let ignoreNextTab = false;
  const all_inputs = getDateInputs();

  all_inputs.forEach((inputs) => {
    const dayInput = inputs.dayInput;
    const monthInput = inputs.monthInput;
    const yearInput = inputs.yearInput;

    updateVisible(inputs);
    dayInput.addEventListener("input", () => {
      ignoreNextTab = handleDayInput(dayInput, monthInput);
      updateActual(inputs);
      setTimeout(() => (ignoreNextTab = false), IGNORE_TAB_DURATION_MS);
    });
    monthInput.addEventListener("input", () => {
      ignoreNextTab = handleMonthInput(monthInput, yearInput);
      updateActual(inputs);
      setTimeout(() => (ignoreNextTab = false), IGNORE_TAB_DURATION_MS);
    });
    yearInput.addEventListener("input", () => {
      handleYearInput(yearInput);
      updateActual(inputs);
      setTimeout(() => (ignoreNextTab = false), IGNORE_TAB_DURATION_MS);
    });

    [monthInput, yearInput].forEach((input) => {
      input.addEventListener(
        "keydown",
        (e) => (ignoreNextTab = tabAdvanceCheck(e, ignoreNextTab)),
      );
    });

    [dayInput, monthInput].forEach((input) => {
      input.addEventListener("blur", () => {
        formatSingleDigit(input);
        updateActual(inputs);
      });
    });
  });
}
